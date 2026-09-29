//! This module is responsible for sending chunks to the client.
//!
//! Chunk sending runs on its own independent tick loop, separate from the game
//! tick. The three-phase design (prepare → encode → commit) minimizes lock hold
//! time on the per-player `ChunkSender` mutex so that game-tick operations like
//! `mark_chunk_pending_to_send` and `drop_chunk` are never blocked for long.
use rayon::{ThreadPool, prelude::*};
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use std::{
    mem,
    sync::{Arc, Weak},
};

use steel_protocol::packet_traits::{ClientPacket, CompressionInfo, EncodedPacket};
use steel_protocol::packets::game::{
    CChunkBatchFinished, CChunkBatchStart, CForgetLevelChunk, CLevelChunkWithLight,
};
use steel_protocol::utils::ConnectionProtocol;
use steel_utils::locks::SyncMutex;
use steel_utils::{ChunkPos, PackedChunkPos};

use crate::{
    chunk::{
        chunk_holder::{ChunkHolder, TickingReadinessSnapshot},
        status::ChunkStatus,
    },
    player::PlayerConnection,
    player::connection::NetworkConnection,
    world::World,
};

/// Minimum chunks per tick (vanilla: 0.01)
const MIN_CHUNKS_PER_TICK: f32 = 0.1f32;
/// Maximum chunks per tick (vanilla: 64.0, we use 500.0 for faster loading)
const MAX_CHUNKS_PER_TICK: f32 = 500.0;
/// Starting chunks per tick (vanilla: 9.0)
const START_CHUNKS_PER_TICK: f32 = 9.0;
/// Maximum unacknowledged batches after first ack (vanilla: 10)
const MAX_UNACKNOWLEDGED_BATCHES: u16 = 10;

/// Connection-wide pacing, shared across world changes and player replacements.
///
/// Unlike vanilla's single-threaded sender, Steel unlocks the sender while encoding.
/// ACKs are queued until the next prepare so they cannot reset an in-flight batch's quota.
#[derive(Debug)]
struct ChunkBatchPacing {
    /// Committed batches whose ACK feedback has not yet been applied.
    unacknowledged_batches: u16,
    /// Client-reported rate, including fractional chunks of credit per sending tick.
    desired_chunks_per_tick: f32,
    /// Accumulated credit; preparation floors this to select whole chunks only.
    batch_quota: f32,
    /// Starts at one batch; the first ACK opens the full send window.
    max_unacknowledged_batches: u16,
    /// Accepted ACK rates in arrival order, bounded by the outstanding batch count.
    accepted_feedback: SmallVec<[f32; MAX_UNACKNOWLEDGED_BATCHES as usize]>,
}

impl ChunkBatchPacing {
    fn begin_prepare(&mut self) -> Option<usize> {
        self.drain_feedback();

        if self.unacknowledged_batches >= self.max_unacknowledged_batches {
            return None;
        }

        let max_batch_size = self.desired_chunks_per_tick.max(1.0);
        self.batch_quota = (self.batch_quota + self.desired_chunks_per_tick).min(max_batch_size);
        Some(self.batch_quota.floor() as usize)
    }

    fn commit_batch(&mut self, batch_size: usize) {
        debug_assert!(batch_size > 0);
        debug_assert!(batch_size as f32 <= self.batch_quota);
        self.unacknowledged_batches += 1;
        self.batch_quota -= batch_size as f32;
    }

    fn record_feedback(&mut self, desired_chunks_per_tick: f32) -> bool {
        let outstanding_batch_count = usize::from(self.unacknowledged_batches);
        if self.accepted_feedback.len() >= outstanding_batch_count
            || self.accepted_feedback.len() >= usize::from(MAX_UNACKNOWLEDGED_BATCHES)
        {
            return false;
        }

        self.accepted_feedback.push(desired_chunks_per_tick);
        true
    }

    fn drain_feedback(&mut self) {
        // Acceptance reserves at most one ACK per outstanding batch. Commits can
        // only increase that count, so it reaches zero only on the final queued ACK.
        debug_assert!(self.accepted_feedback.len() <= usize::from(self.unacknowledged_batches));
        for desired_chunks_per_tick in mem::take(&mut self.accepted_feedback) {
            self.unacknowledged_batches = self.unacknowledged_batches.saturating_sub(1);
            self.desired_chunks_per_tick = if desired_chunks_per_tick.is_nan() {
                MIN_CHUNKS_PER_TICK
            } else {
                desired_chunks_per_tick.clamp(MIN_CHUNKS_PER_TICK, MAX_CHUNKS_PER_TICK)
            };

            if self.unacknowledged_batches == 0 {
                self.batch_quota = 1.0;
            }

            self.max_unacknowledged_batches = MAX_UNACKNOWLEDGED_BATCHES;
        }
    }
}

impl Default for ChunkBatchPacing {
    fn default() -> Self {
        Self {
            unacknowledged_batches: 0,
            desired_chunks_per_tick: START_CHUNKS_PER_TICK,
            batch_quota: 0.0,
            max_unacknowledged_batches: 1,
            accepted_feedback: SmallVec::new(),
        }
    }
}

/// One chunk selected during the prepare phase.
pub struct PreparedChunk {
    /// Chunk position.
    pub pos: ChunkPos,
    /// Chunk holder to encode.
    pub holder: Arc<ChunkHolder>,
    /// Exact readiness generation observed while selecting the holder.
    readiness: TickingReadinessSnapshot,
}

/// Data collected during the prepare phase, used to encode and then commit.
pub struct PreparedBatch {
    /// Chunk holders to encode.
    pub chunks: Vec<PreparedChunk>,
    /// Whether the world dimension has a vanilla sky-light layer.
    pub has_skylight: bool,
    /// Snapshot of the player's generation counter at prepare time.
    pub epoch_snapshot: u32,
}

/// Encoded chunk packet plus the holder and readiness generation it was built from.
#[derive(Clone)]
pub struct EncodedChunk {
    pos: ChunkPos,
    packet: EncodedPacket,
    content_revision: u64,
    holder: Weak<ChunkHolder>,
    readiness: TickingReadinessSnapshot,
}

impl EncodedChunk {
    fn is_current_for(&self, prepared: &PreparedChunk) -> bool {
        let Some(encoded_holder) = self.holder.upgrade() else {
            return false;
        };

        self.pos == prepared.pos
            && Arc::ptr_eq(&encoded_holder, &prepared.holder)
            && self.readiness == prepared.readiness
            && prepared.readiness.is_block_ticking()
            && prepared.holder.ticking_readiness_snapshot() == prepared.readiness
            && prepared.holder.packet_content_revision() == self.content_revision
    }
}

/// This struct is responsible for sending chunks to the client.
#[derive(Debug, Default)]
pub struct ChunkSender {
    /// A list of chunks that are waiting to be sent to the client.
    pub pending_chunks: FxHashSet<ChunkPos>,
    /// Chunks whose initial chunk packet has been queued for this client.
    sent_chunks: FxHashSet<ChunkPos>,
    pacing: ChunkBatchPacing,
}

impl ChunkSender {
    /// Clears chunk membership from the previous world while preserving connection pacing.
    pub(crate) fn clear_world_chunks(&mut self) {
        self.pending_chunks.clear();
        self.sent_chunks.clear();
    }

    /// Marks a chunk as pending to be sent to the client.
    pub fn mark_chunk_pending_to_send(&mut self, pos: ChunkPos) {
        self.sent_chunks.remove(&pos);
        self.pending_chunks.insert(pos);
    }

    /// Drops a chunk from the client's view.
    pub fn drop_chunk(&mut self, connection: &PlayerConnection, pos: ChunkPos) {
        self.pending_chunks.remove(&pos);
        if self.sent_chunks.remove(&pos) && !connection.closed() {
            Self::send_packet(
                connection,
                CForgetLevelChunk {
                    pos: PackedChunkPos::from(pos),
                },
            );
        }
    }

    fn send_packet<P: ClientPacket>(connection: &PlayerConnection, packet: P) {
        let encoded =
            EncodedPacket::from_bare(packet, connection.compression(), ConnectionProtocol::Play)
                .expect("Failed to encode packet");
        connection.send_encoded(encoded);
    }

    /// Phase 1: Lock briefly to drain pending chunks and snapshot state.
    ///
    /// Returns `None` if there is nothing to send this tick.
    /// The caller must complete or discard the returned batch before preparing
    /// another one for this sender; the server's sending pass enforces this.
    pub fn prepare_batch(
        &mut self,
        world: &Arc<World>,
        player_chunk_pos: ChunkPos,
        chunk_send_epoch: &SyncMutex<u32>,
    ) -> Option<PreparedBatch> {
        let max_batch_size = self.pacing.begin_prepare()?;
        if max_batch_size == 0 || self.pending_chunks.is_empty() {
            return None;
        }

        let holders = self.collect_candidates(world, player_chunk_pos, max_batch_size);
        if holders.is_empty() {
            return None;
        }

        let epoch_snapshot = *chunk_send_epoch.lock();

        Some(PreparedBatch {
            chunks: holders,
            has_skylight: world.dimension_type.has_skylight,
            epoch_snapshot,
        })
    }

    /// Phase 2: Encode chunks without holding any lock. Called between prepare and commit.
    ///
    /// Uses the dedicated encoding pool to encode chunks in parallel. A per-tick
    /// local cache prevents multiple players sharing the same chunks from
    /// re-encoding them within the same sending tick.
    ///
    /// # Panics
    /// Panics if a chunk packet fails to encode.
    pub fn encode_batch(
        batch: &PreparedBatch,
        cache: &mut FxHashMap<ChunkPos, EncodedChunk>,
        compression: Option<CompressionInfo>,
        encoding_pool: &ThreadPool,
    ) -> Vec<EncodedChunk> {
        let cached_chunks = &*cache;
        let encoded_chunks = encoding_pool.install(|| {
            batch
                .chunks
                .par_iter()
                .map(|prepared| {
                    let holder = &prepared.holder;
                    let pos = prepared.pos;

                    if let Some(cached) = cached_chunks.get(&pos)
                        && cached.is_current_for(prepared)
                    {
                        return Some(cached.clone());
                    }

                    if !prepared.readiness.is_block_ticking()
                        || holder.ticking_readiness_snapshot() != prepared.readiness
                    {
                        return None;
                    }
                    let revision_before = holder.packet_content_revision();
                    let chunk = holder.try_full_chunk()?;

                    let packet = EncodedPacket::from_bare(
                        CLevelChunkWithLight {
                            x: pos.0.x,
                            z: pos.0.y,
                            chunk_data: chunk.extract_chunk_data(),
                            light_data: chunk.extract_light_data(batch.has_skylight),
                        },
                        compression,
                        ConnectionProtocol::Play,
                    )
                    .expect("Failed to encode chunk packet");
                    let revision_after = holder.packet_content_revision();
                    if revision_before != revision_after
                        || holder.ticking_readiness_snapshot() != prepared.readiness
                    {
                        return None;
                    }

                    Some(EncodedChunk {
                        pos,
                        packet,
                        content_revision: revision_after,
                        holder: Arc::downgrade(holder),
                        readiness: prepared.readiness,
                    })
                })
                .collect::<Vec<_>>()
        });
        let encoded_chunks = encoded_chunks.into_iter().flatten().collect::<Vec<_>>();

        for encoded in &encoded_chunks {
            cache.insert(encoded.pos, encoded.clone());
        }

        encoded_chunks
    }

    /// Phase 3: Lock briefly to verify generation counter and send the batch.
    ///
    /// If the player teleported between prepare and commit (generation counter
    /// changed), the batch is discarded.
    pub fn commit_batch(
        &mut self,
        batch: &PreparedBatch,
        encoded_chunks: Vec<EncodedChunk>,
        connection: &PlayerConnection,
        chunk_send_epoch: &SyncMutex<u32>,
    ) -> Vec<ChunkPos> {
        let epoch = chunk_send_epoch.lock();
        if *epoch != batch.epoch_snapshot {
            return Vec::new();
        }
        drop(epoch);

        let valid_chunks =
            Self::resolve_valid_chunks(&batch.chunks, encoded_chunks, &self.pending_chunks);

        if valid_chunks.is_empty() {
            return Vec::new();
        }

        self.pacing.commit_batch(valid_chunks.len());

        Self::send_packet(connection, CChunkBatchStart {});

        let batch_size = valid_chunks.len();
        for encoded in &valid_chunks {
            connection.send_encoded(encoded.packet.clone());
        }

        Self::send_packet(
            connection,
            CChunkBatchFinished {
                batch_size: batch_size as i32,
            },
        );

        let mut sent_chunks = Vec::with_capacity(valid_chunks.len());
        for encoded in valid_chunks {
            self.pending_chunks.remove(&encoded.pos);
            self.sent_chunks.insert(encoded.pos);
            sent_chunks.push(encoded.pos);
        }
        sent_chunks
    }

    /// Keeps the encoded chunks that are still pending and still match their prepared source.
    ///
    /// `encoded_chunks` is a subsequence of `prepared` in the same relative order:
    /// [`Self::encode_batch`] maps over `prepared` with an order-preserving parallel
    /// iterator and only drops entries. A single forward cursor over `prepared` therefore
    /// resolves every encoded chunk in one pass, instead of restarting the scan per chunk.
    fn resolve_valid_chunks(
        prepared: &[PreparedChunk],
        encoded_chunks: Vec<EncodedChunk>,
        pending: &FxHashSet<ChunkPos>,
    ) -> Vec<EncodedChunk> {
        let mut prepared_chunks = prepared.iter();
        let mut valid_chunks = Vec::with_capacity(encoded_chunks.len());

        for encoded in encoded_chunks {
            if !pending.contains(&encoded.pos) {
                continue;
            }
            let Some(prepared) = prepared_chunks.find(|prepared| prepared.pos == encoded.pos)
            else {
                continue;
            };
            if encoded.is_current_for(prepared) {
                valid_chunks.push(encoded);
            }
        }

        valid_chunks
    }

    fn collect_candidates(
        &mut self,
        world: &Arc<World>,
        player_chunk_pos: ChunkPos,
        max_batch_size: usize,
    ) -> Vec<PreparedChunk> {
        let mut candidates: Vec<ChunkPos> = self.pending_chunks.iter().copied().collect();

        candidates.sort_by_key(|pos| Self::chunk_distance_squared(*pos, player_chunk_pos));

        let mut chunks_to_send = Vec::new();

        for pos in candidates {
            if chunks_to_send.len() >= max_batch_size {
                break;
            }

            if let Some(holder) = world
                .chunk_map
                .chunks
                .read_sync(&pos, |_, chunk| chunk.clone())
                && holder.published_status() == Some(ChunkStatus::Full)
            {
                let readiness = holder.ticking_readiness_snapshot();
                if readiness.is_block_ticking() {
                    chunks_to_send.push(PreparedChunk {
                        pos,
                        holder,
                        readiness,
                    });
                }
            }
        }
        chunks_to_send
    }

    fn chunk_distance_squared(pos: ChunkPos, player_chunk_pos: ChunkPos) -> u64 {
        let dx = u64::from(pos.0.x.abs_diff(player_chunk_pos.0.x));
        let dz = u64::from(pos.0.y.abs_diff(player_chunk_pos.0.y));
        dx.saturating_mul(dx).saturating_add(dz.saturating_mul(dz))
    }

    /// Queues accepted client rate feedback for the next prepare boundary.
    pub fn on_chunk_batch_received_by_client(&mut self, desired_chunks_per_tick: f32) -> bool {
        self.pacing.record_feedback(desired_chunks_per_tick)
    }

    /// Returns whether the client has been queued the initial chunk packet.
    #[must_use]
    pub fn is_chunk_sent(&self, pos: ChunkPos) -> bool {
        self.sent_chunks.contains(&pos)
    }

    /// Returns a snapshot of all sent chunks for this player.
    #[must_use]
    pub fn sent_chunks_snapshot(&self) -> FxHashSet<ChunkPos> {
        self.sent_chunks.clone()
    }

    #[cfg(test)]
    pub(crate) fn mark_chunk_sent_for_test(&mut self, pos: ChunkPos) {
        self.pending_chunks.remove(&pos);
        self.sent_chunks.insert(pos);
    }

    #[cfg(test)]
    pub(crate) const fn unacknowledged_batch_count_for_test(&self) -> u16 {
        self.pacing.unacknowledged_batches
    }
}

#[cfg(any(test, feature = "benchmark-support"))]
/// Fixtures and direct entry points shared by unit tests and Criterion benchmarks.
///
/// The commit phase needs a [`PlayerConnection`] and cannot be driven from a
/// benchmark, so this exposes the pure batch-resolution step that runs inside it
/// along with the pieces needed to build a realistic batch.
pub mod benchmark_support {
    use std::sync::{Arc, Weak};

    use rustc_hash::FxHashSet;
    use steel_utils::ChunkPos;

    use super::{ChunkSender, EncodedChunk, PreparedChunk};
    use crate::chunk::{
        Chunk,
        chunk_holder::{ChunkHolder, TickingReadiness},
        chunk_ticket_manager::ChunkTicketLevel,
        heightmap::ChunkHeightmaps,
        light::ChunkLightData,
        section::{ChunkSection, Sections},
        status::ChunkStatus,
    };
    use crate::world::tick_scheduler::{BlockTickList, FluidTickList};
    use steel_worldgen::structure::{StructureReferenceMap, StructureStartMap};
    /// Builds a block-ticking prepared chunk backed by a real empty full chunk.
    ///
    /// The holder is the same shape the prepare phase produces, so an encoded
    /// chunk built from it passes every [`EncodedChunk::is_current_for`] check.
    #[must_use]
    pub fn prepared_full_chunk(pos: ChunkPos) -> PreparedChunk {
        let chunk = Chunk::from_full_disk(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            pos,
            0,
            16,
            Weak::new(),
            BlockTickList::new(),
            FluidTickList::new(),
            ChunkHeightmaps::new(0, 16),
            Vec::new(),
            StructureStartMap::default(),
            StructureReferenceMap::default(),
            ChunkLightData::for_valid_world_height(0, 16),
        );
        let holder = Arc::new(ChunkHolder::new(
            pos,
            ChunkTicketLevel::FULL_CHUNK,
            Some(ChunkTicketLevel::FULL_CHUNK),
            0,
            16,
        ));
        holder.insert_chunk(chunk, ChunkStatus::Full);
        holder.transition_ticking_readiness(TickingReadiness::BlockTicking);
        let readiness = holder.ticking_readiness_snapshot();
        PreparedChunk {
            pos,
            holder,
            readiness,
        }
    }

    /// Position the encoded chunk was built for.
    #[must_use]
    pub const fn encoded_pos(encoded: &EncodedChunk) -> ChunkPos {
        encoded.pos
    }

    /// Whether an encoded chunk still matches the prepared chunk it came from.
    ///
    /// Exposed so a benchmark can drive an alternative resolution strategy through
    /// the exact validity check the commit phase uses.
    #[must_use]
    pub fn encoded_is_current_for(encoded: &EncodedChunk, prepared: &PreparedChunk) -> bool {
        encoded.is_current_for(prepared)
    }

    /// Runs the batch-resolution step of [`ChunkSender::commit_batch`].
    #[must_use]
    #[expect(
        clippy::implicit_hasher,
        reason = "mirrors the FxHashSet the commit phase actually passes"
    )]
    pub fn resolve_valid_chunks(
        prepared: &[PreparedChunk],
        encoded_chunks: Vec<EncodedChunk>,
        pending: &FxHashSet<ChunkPos>,
    ) -> Vec<EncodedChunk> {
        ChunkSender::resolve_valid_chunks(prepared, encoded_chunks, pending)
    }
}

#[cfg(test)]
mod tests;
