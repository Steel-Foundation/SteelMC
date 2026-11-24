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
    pub(super) fn get_loaded(world: &World, pos: ChunkPos, status: ChunkStatus) -> Option<Self> {
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

    /// Uses an existing chunk's permitted generation work without adding a ticket.
    pub(super) async fn load_if_present(
        world: &Arc<World>,
        pos: ChunkPos,
        status: ChunkStatus,
    ) -> Option<Self> {
        let holder = world
            .chunk_map
            .chunks
            .read_sync(&pos, |_, holder| Arc::clone(holder))?;
        if holder.is_status_disallowed(status) {
            return None;
        }
        if holder.try_chunk(status).is_none() {
            holder.schedule_chunk_generation_task_b(status, &world.chunk_map);
            holder.await_chunk(status).await?;
        }
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
        Self::from_request(request).await
    }

    pub(super) async fn from_request(request: ChunkRequestHandle) -> io::Result<Self> {
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
