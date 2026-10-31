//! Simulation propagation stops at the block-ticking boundary.

use super::{chunk_ticket_manager::ChunkTicketLevel, chunk_tracker::ChunkTracker};

pub use super::chunk_tracker::ChunkLevelChange as SimulationLevelChange;

/// Vanilla's simulation tracker, with an independent graph and ticking limit.
pub type SimulationTicketManager = ChunkTracker<{ ChunkTicketLevel::BLOCK_TICKING_CHUNK.raw() }>;
