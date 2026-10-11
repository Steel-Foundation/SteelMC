use std::sync::Arc;

use steel_utils::ChunkPos;

use crate::chunk::chunk_request::ChunkRequestState;
use crate::world::World;

use super::ChunkRequestHandle;

/// The chunk the search is currently reading.
pub(super) struct ChunkLoad {
    pub(super) chunk: ChunkPos,
    /// `None` when the chunk is held by the prefetch, which keeps it loaded already.
    request: Option<ChunkRequestHandle>,
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
    pub(super) fn open(world: &Arc<World>, chunk: ChunkPos, next_scan: i32, held: bool) -> Self {
        Self {
            chunk,
            request: (!held).then(|| world.request_spawn_candidate_chunk(chunk)),
            next_scan,
        }
    }

    /// Whether this load made its own chunk request.
    pub(super) const fn made_request(&self) -> bool {
        self.request.is_some()
    }

    pub(super) fn poll(&self, world: &Arc<World>) -> ChunkLoadState {
        let Some(request) = &self.request else {
            let full = world
                .chunk_map
                .with_full_chunk(self.chunk, |_| ())
                .is_some();
            return if full {
                ChunkLoadState::Ready
            } else {
                ChunkLoadState::Waiting
            };
        };
        match request.poll() {
            ChunkRequestState::Cancelled => ChunkLoadState::Cancelled,
            ChunkRequestState::Ready if request.ready_chunks().is_some() => ChunkLoadState::Ready,
            ChunkRequestState::Pending { .. } | ChunkRequestState::Ready => ChunkLoadState::Waiting,
        }
    }
}
