//! This module is responsible for sending chunks to the client.
//!
//! Chunk sending runs on its own independent tick loop, separate from the game
//! tick. The three-phase design (prepare → encode → commit) minimizes lock hold
//! time on the per-player `ChunkSender` mutex so that game-tick operations like
//! `mark_chunk_pending_to_send` and `drop_chunk` are never blocked for long.
use rustc_hash::FxHashSet;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use steel_protocol::packet_traits::{ClientPacket, CompressionInfo, EncodedPacket};
use steel_protocol::packets::game::{
    CChunkBatchFinished, CChunkBatchStart, CForgetLevelChunk, CLevelChunkWithLight,
};
use steel_protocol::utils::ConnectionProtocol;
use steel_utils::ChunkPos;

use crate::{
    chunk::{
        chunk_access::{ChunkAccess, ChunkStatus},
        chunk_holder::ChunkHolder,
    },
    player::PlayerConnection,
    player::connection::NetworkConnection,
    world::World,
};

/// Minimum chunks per tick (vanilla: 0.01)
const MIN_CHUNKS_PER_TICK: f32 = 8f32;
/// Maximum chunks per tick (vanilla: 64.0, we use 500.0 for faster loading)
const MAX_CHUNKS_PER_TICK: f32 = 500.0;
/// Starting chunks per tick (vanilla: 9.0)
const START_CHUNKS_PER_TICK: f32 = 9.0;
/// Maximum unacknowledged batches after first ack (vanilla: 10)
const MAX_UNACKNOWLEDGED_BATCHES: u16 = 10;

/// Data collected during the prepare phase, used to encode and then commit.
pub struct PreparedBatch {
    /// Chunk holders to encode.
    pub holders: Vec<Arc<ChunkHolder>>,
    /// Snapshot of the player's generation counter at prepare time.
    pub epoch_snapshot: u32,
}

/// This struct is responsible for sending chunks to the client.
#[derive(Debug)]
pub struct ChunkSender {
    /// A list of chunks that are waiting to be sent to the client.
    pub pending_chunks: FxHashSet<ChunkPos>,
    /// The number of batches that have been sent to the client but have not been acknowledged yet.
    pub unacknowledged_batches: u16,
    /// The number of chunks that should be sent to the client per tick.
    /// This is dynamically adjusted based on client feedback.
    pub desired_chunks_per_tick: f32,
    /// The number of chunks that can be sent to the client in the current batch.
    pub batch_quota: f32,
    /// The maximum number of unacknowledged batches allowed.
    /// Starts at 1 and increases to `MAX_UNACKNOWLEDGED_BATCHES` after first ack.
    pub max_unacknowledged_batches: u16,
}

impl ChunkSender {
    /// Marks a chunk as pending to be sent to the client.
    pub fn mark_chunk_pending_to_send(&mut self, pos: ChunkPos) {
        self.pending_chunks.insert(pos);
    }

    /// Drops a chunk from the client's view.
    pub fn drop_chunk(&mut self, connection: &PlayerConnection, pos: ChunkPos) {
        if !self.pending_chunks.remove(&pos) && !connection.closed() {
            Self::send_packet(connection, CForgetLevelChunk { pos });
        }
    }

    /// Encodes and sends a packet through the connection.
    fn send_packet<P: ClientPacket>(connection: &PlayerConnection, packet: P) {
        let encoded =
            EncodedPacket::from_bare(packet, connection.compression(), ConnectionProtocol::Play)
                .expect("Failed to encode packet");
        connection.send_encoded(encoded);
    }

    /// Phase 1: Lock briefly to drain pending chunks and snapshot state.
    ///
    /// Returns `None` if there is nothing to send this tick.
    pub fn prepare_batch(
        &mut self,
        world: &Arc<World>,
        player_chunk_pos: ChunkPos,
        chunk_send_epoch: &AtomicU32,
    ) -> Option<PreparedBatch> {
        if self.unacknowledged_batches >= self.max_unacknowledged_batches {
            return None;
        }

        let max_batch_size = self.desired_chunks_per_tick.max(1.0);
        self.batch_quota = (self.batch_quota + self.desired_chunks_per_tick).min(max_batch_size);

        if self.batch_quota < 1.0 || self.pending_chunks.is_empty() {
            return None;
        }

        let holders = self.collect_candidates(world, player_chunk_pos);
        if holders.is_empty() {
            return None;
        }

        let epoch_snapshot = chunk_send_epoch.load(Ordering::Acquire);

        Some(PreparedBatch {
            holders,
            epoch_snapshot,
        })
    }

    /// Phase 2: Encode chunks without holding any lock. Called between prepare and commit.
    ///
    /// Uses a per-tick local cache so multiple players sharing the same chunks
    /// don't re-encode them within the same sending tick. No mutex needed.
    ///
    /// # Panics
    /// Panics if a chunk packet fails to encode.
    pub fn encode_batch(
        batch: &PreparedBatch,
        cache: &mut rustc_hash::FxHashMap<ChunkPos, EncodedPacket>,
        compression: Option<CompressionInfo>,
    ) -> Vec<EncodedPacket> {
        let mut encoded_chunks = Vec::with_capacity(batch.holders.len());

        for holder in &batch.holders {
            let pos = ChunkPos::new(holder.get_pos().0.x, holder.get_pos().0.y);

            if let Some(cached) = cache.get(&pos) {
                encoded_chunks.push(cached.clone());
                continue;
            }

            let Some(chunk_guard) = holder.try_chunk(ChunkStatus::Full) else {
                continue;
            };
            let ChunkAccess::Full(chunk) = &*chunk_guard else {
                continue;
            };

            let encoded = EncodedPacket::from_bare(
                CLevelChunkWithLight {
                    x: pos.0.x,
                    z: pos.0.y,
                    chunk_data: chunk.extract_chunk_data(),
                    light_data: chunk.extract_light_data(),
                },
                compression,
                ConnectionProtocol::Play,
            )
            .expect("Failed to encode chunk packet");

            cache.insert(pos, encoded.clone());
            encoded_chunks.push(encoded);
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
        encoded_chunks: Vec<EncodedPacket>,
        connection: &PlayerConnection,
        chunk_send_epoch: &AtomicU32,
    ) {
        if chunk_send_epoch.load(Ordering::Acquire) != batch.epoch_snapshot {
            return;
        }

        if encoded_chunks.is_empty() {
            return;
        }

        self.unacknowledged_batches += 1;
        self.batch_quota -= encoded_chunks.len() as f32;

        Self::send_packet(connection, CChunkBatchStart {});

        let batch_size = encoded_chunks.len();
        for encoded in encoded_chunks {
            connection.send_encoded(encoded);
        }

        Self::send_packet(
            connection,
            CChunkBatchFinished {
                batch_size: batch_size as i32,
            },
        );
    }

    fn collect_candidates(
        &mut self,
        world: &Arc<World>,
        player_chunk_pos: ChunkPos,
    ) -> Vec<Arc<ChunkHolder>> {
        let max_batch_size = self.batch_quota.floor() as usize;
        let mut candidates: Vec<ChunkPos> = self.pending_chunks.iter().copied().collect();

        // Sort by distance to player
        candidates.sort_by_key(|pos| {
            let dx = pos.0.x - player_chunk_pos.0.x;
            let dz = pos.0.y - player_chunk_pos.0.y;
            dx * dx + dz * dz
        });

        let mut chunks_to_send = Vec::new();

        for pos in candidates {
            if chunks_to_send.len() >= max_batch_size {
                break;
            }

            if let Some(holder) = world
                .chunk_map
                .chunks
                .read_sync(&pos, |_, chunk| chunk.clone())
                && holder.persisted_status() == Some(ChunkStatus::Full)
            {
                chunks_to_send.push(holder);
                self.pending_chunks.remove(&pos);
            }
        }
        chunks_to_send
    }

    /// Handles the acknowledgement of a chunk batch from the client.
    ///
    /// The client sends back its desired chunks per tick based on how fast it can
    /// process chunks. We clamp this value and use it to adjust our sending rate.
    pub const fn on_chunk_batch_received_by_client(&mut self, desired_chunks_per_tick: f32) {
        self.unacknowledged_batches = self.unacknowledged_batches.saturating_sub(1);

        // Handle NaN and clamp to valid range (vanilla uses 0.01-64, we use 0.01-500)
        self.desired_chunks_per_tick = if desired_chunks_per_tick.is_nan() {
            MIN_CHUNKS_PER_TICK
        } else {
            desired_chunks_per_tick.clamp(MIN_CHUNKS_PER_TICK, MAX_CHUNKS_PER_TICK)
        };

        // Reset batch quota when all batches are acknowledged
        if self.unacknowledged_batches == 0 {
            self.batch_quota = 1.0;
        }

        // After receiving the first acknowledgement, allow more unacknowledged batches
        // for better pipelining (vanilla behavior)
        self.max_unacknowledged_batches = MAX_UNACKNOWLEDGED_BATCHES;
    }
}

impl Default for ChunkSender {
    fn default() -> Self {
        Self {
            pending_chunks: FxHashSet::default(),
            unacknowledged_batches: 0,
            desired_chunks_per_tick: START_CHUNKS_PER_TICK,
            batch_quota: 0.0,
            max_unacknowledged_batches: 1,
        }
    }
}
