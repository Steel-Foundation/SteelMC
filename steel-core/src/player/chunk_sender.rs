//! This module is responsible for sending chunks to the client.
use rustc_hash::FxHashSet;
use std::sync::Arc;

use steel_protocol::packet_traits::{ClientPacket, EncodedPacket};
use steel_protocol::packets::game::{
    CChunkBatchFinished, CChunkBatchStart, CForgetLevelChunk, CLevelChunkWithLight,
};
use steel_protocol::utils::ConnectionProtocol;
use steel_utils::ChunkPos;
use tokio::task::spawn_blocking;

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

    /// Sends the next batch of chunks to the client.
    ///
    /// # Panics
    /// Panics if a chunk is not at Full status when it should be.
    pub fn send_next_chunks(
        &mut self,
        connection: Arc<PlayerConnection>,
        world: &World,
        player_chunk_pos: ChunkPos,
    ) {
        if self.unacknowledged_batches < self.max_unacknowledged_batches {
            let max_batch_size = self.desired_chunks_per_tick.max(1.0);
            self.batch_quota =
                (self.batch_quota + self.desired_chunks_per_tick).min(max_batch_size);

            if self.batch_quota >= 1.0 && !self.pending_chunks.is_empty() {
                let chunks_to_process = self.collect_candidates(world, player_chunk_pos);
                if !chunks_to_process.is_empty() {
                    self.unacknowledged_batches += 1;
                    self.batch_quota -= chunks_to_process.len() as f32;

                    // Pre-compute compression info for encoding inside the blocking task
                    let compression = connection.compression();

                    #[allow(clippy::let_underscore_future)]
                    let _ = spawn_blocking(move || {
                        let mut chunks_to_send = Vec::new();
                        for holder in chunks_to_process {
                            if let Some(chunk_guard) = holder.try_chunk(ChunkStatus::Full) {
                                if let ChunkAccess::Full(chunk) = &*chunk_guard {
                                    chunks_to_send.push(CLevelChunkWithLight {
                                        pos: holder.get_pos(),
                                        chunk_data: chunk.extract_chunk_data(),
                                        light_data: chunk.extract_light_data(),
                                    });
                                } else {
                                    panic!("Chunk must be at Full status to be sent to the client");
                                }
                            }
                        }

                        // Encode and send batch start
                        let start_encoded = EncodedPacket::from_bare(
                            CChunkBatchStart {},
                            compression,
                            ConnectionProtocol::Play,
                        )
                        .expect("Failed to encode packet");
                        connection.send_encoded(start_encoded);

                        let batch_size = chunks_to_send.len();

                        for chunk in chunks_to_send {
                            let chunk_encoded = EncodedPacket::from_bare(
                                chunk,
                                compression,
                                ConnectionProtocol::Play,
                            )
                            .expect("Failed to encode chunk packet");
                            connection.send_encoded(chunk_encoded);
                        }

                        let finish_encoded = EncodedPacket::from_bare(
                            CChunkBatchFinished {
                                batch_size: batch_size as i32,
                            },
                            compression,
                            ConnectionProtocol::Play,
                        )
                        .expect("Failed to encode packet");
                        connection.send_encoded(finish_encoded);
                    });
                }
            }
        }
    }

    fn collect_candidates(
        &mut self,
        world: &World,
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
