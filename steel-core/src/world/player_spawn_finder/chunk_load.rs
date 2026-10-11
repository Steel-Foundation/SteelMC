use std::sync::Arc;

use steel_utils::ChunkPos;

use crate::chunk::chunk_request::ChunkRequestState;
use crate::world::World;

use super::ChunkRequestHandle;

/// The chunk the search is currently reading.
pub(super) struct ChunkLoad {
    pub(super) chunk: ChunkPos,
    request: ChunkRequestHandle,
    /// Next sequence index to look at. Candidates before the pending one are already
    /// consumed, so the scan starts at the pending candidate.
    pub(super) next_scan: i32,
}

#[derive(Clone, Copy)]
pub(super) enum ChunkLoadState {
    Ready,
    Waiting,
    Cancelled,
}

impl ChunkLoad {
    pub(super) fn open(world: &Arc<World>, chunk: ChunkPos, next_scan: i32) -> Self {
        Self {
            chunk,
            request: world.request_spawn_candidate_chunk(chunk),
            next_scan,
        }
    }

    pub(super) fn poll(&self) -> ChunkLoadState {
        match self.request.poll() {
            ChunkRequestState::Cancelled => ChunkLoadState::Cancelled,
            ChunkRequestState::Ready if self.request.ready_chunks().is_some() => {
                ChunkLoadState::Ready
            }
            ChunkRequestState::Pending { .. } | ChunkRequestState::Ready => ChunkLoadState::Waiting,
        }
    }
}
