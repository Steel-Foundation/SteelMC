use std::{future::poll_fn, io, sync::Arc, task::Poll};

use steel_utils::ChunkPos;

use crate::{
    chunk::{
        chunk_holder::ChunkHolder,
        chunk_request::{ChunkRequestHandle, ChunkRequestState, ChunkTicketKind},
        status::ChunkStatus,
    },
    world::World,
};

/// Retains a loaded holder and any ticket used to obtain it while map data is read.
pub(super) struct MapChunk {
    pub(super) holder: Arc<ChunkHolder>,
    _request: Option<ChunkRequestHandle>,
}

impl MapChunk {
    fn get_loaded(world: &World, pos: ChunkPos, status: ChunkStatus) -> Option<Self> {
        let holder = world
            .chunk_map
            .chunks
            .read_sync(&pos, |_, holder| Arc::clone(holder))?;
        holder.try_chunk(status)?;
        // Retaining the holder prevents unload finalization while it is read.
        Some(Self {
            holder,
            _request: None,
        })
    }

    pub(super) async fn load(
        world: &Arc<World>,
        pos: ChunkPos,
        status: ChunkStatus,
    ) -> io::Result<Self> {
        if let Some(chunk) = Self::get_loaded(world, pos, status) {
            return Ok(chunk);
        }
        let request = world
            .chunk_map
            .request_chunk(pos, status, ChunkTicketKind::Map);
        poll_fn(|_| match request.poll() {
            ChunkRequestState::Pending { .. } => Poll::Pending,
            ChunkRequestState::Cancelled => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "map chunk request was cancelled",
            ))),
            ChunkRequestState::Ready => Poll::Ready(Ok(())),
        })
        .await?;
        let Some(ready) = request.ready_chunks() else {
            return Err(io::Error::other("ready map request lost its chunk"));
        };
        let Some(holder) = ready.holders.into_iter().next() else {
            return Err(io::Error::other("map request returned no chunk"));
        };
        Ok(Self {
            holder,
            _request: Some(request),
        })
    }
}
