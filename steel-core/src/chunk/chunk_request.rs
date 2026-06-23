//! Ticket-owned chunk availability requests.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use steel_utils::ChunkPos;

use crate::chunk::{
    chunk_access::ChunkStatus,
    chunk_holder::ChunkHolder,
    chunk_map::ChunkMap,
    chunk_ticket_manager::{ChunkTicket, ticket_level_for_status},
};

/// Why a chunk request is holding tickets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkTicketKind {
    /// Player-visible chunk loading.
    Player,
    /// Initial chunks around a joining player's spawn.
    PlayerSpawn,
    /// Candidate chunks loaded while searching for a valid spawn position.
    SpawnSearch,
    /// Candidate chunks loaded by structure location queries.
    StructureLocate,
    /// Chunks loaded by startup pregeneration.
    Pregen,
    /// Generic command-owned chunk request.
    Command,
}

/// Request for a set of chunks at a minimum generation status.
pub struct ChunkRequest {
    /// Minimum chunk status required before the request is ready.
    pub status: ChunkStatus,
    /// Chunk positions requested.
    pub positions: Vec<ChunkPos>,
    /// Ticket owner category.
    pub ticket_kind: ChunkTicketKind,
}

/// Poll result for a [`ChunkRequestHandle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkRequestState {
    /// The request has outstanding chunks.
    Pending {
        /// Number of requested chunks already at the target status.
        ready: usize,
        /// Number of requested chunks after deduplication.
        total: usize,
    },
    /// Every requested chunk is available at the target status.
    Ready,
    /// The request was cancelled and no longer owns tickets.
    Cancelled,
}

/// Chunks passed to request continuations once a request is ready.
pub struct ReadyChunks {
    /// Status all holders have reached.
    pub status: ChunkStatus,
    /// Holders for the requested positions.
    pub holders: Vec<Arc<ChunkHolder>>,
}

struct ChunkRequestInner {
    chunk_map: Arc<ChunkMap>,
    positions: Box<[ChunkPos]>,
    status: ChunkStatus,
    ticket_kind: ChunkTicketKind,
    ticket: ChunkTicket,
}

/// Handle for a ticketed chunk request.
///
/// Dropping or cancelling the handle releases its tickets. The handle never
/// creates chunk holders directly; holder creation remains owned by the normal
/// chunk scheduling tick.
pub struct ChunkRequestHandle {
    inner: Option<ChunkRequestInner>,
}

impl ChunkRequestHandle {
    pub(crate) fn new(chunk_map: Arc<ChunkMap>, request: ChunkRequest) -> Self {
        let ticket = ChunkTicket::loading(ticket_level_for_status(request.status));
        Self::new_with_ticket(chunk_map, request, ticket)
    }

    fn new_with_ticket(
        chunk_map: Arc<ChunkMap>,
        request: ChunkRequest,
        ticket: ChunkTicket,
    ) -> Self {
        let positions = dedupe_positions(request.positions);

        {
            let mut tickets = chunk_map.chunk_tickets.lock();
            for &pos in &positions {
                tickets.add_ticket(pos, ticket);
            }
        }

        Self {
            inner: Some(ChunkRequestInner {
                chunk_map,
                positions,
                status: request.status,
                ticket_kind: request.ticket_kind,
                ticket,
            }),
        }
    }

    /// Returns the requested status, if this handle is still active.
    #[must_use]
    pub fn status(&self) -> Option<ChunkStatus> {
        self.inner.as_ref().map(|inner| inner.status)
    }

    /// Returns the ticket kind, if this handle is still active.
    #[must_use]
    pub fn ticket_kind(&self) -> Option<ChunkTicketKind> {
        self.inner.as_ref().map(|inner| inner.ticket_kind)
    }

    /// Returns requested positions after deduplication.
    #[must_use]
    pub fn positions(&self) -> &[ChunkPos] {
        self.inner
            .as_ref()
            .map_or(&[], |inner| inner.positions.as_ref())
    }

    /// Polls request readiness. Chunk holder creation and generation scheduling
    /// are owned by the chunk scheduling tick.
    #[must_use]
    pub fn poll(&self) -> ChunkRequestState {
        let Some(inner) = &self.inner else {
            return ChunkRequestState::Cancelled;
        };
        if inner.positions.is_empty() {
            return ChunkRequestState::Ready;
        }

        let mut ready = 0;
        for &pos in &inner.positions {
            let Some(holder) = inner
                .chunk_map
                .chunks
                .read_sync(&pos, |_, holder| holder.clone())
            else {
                continue;
            };

            if holder.try_chunk(inner.status).is_some() {
                ready += 1;
            }
        }

        if ready == inner.positions.len() {
            ChunkRequestState::Ready
        } else {
            ChunkRequestState::Pending {
                ready,
                total: inner.positions.len(),
            }
        }
    }

    /// Returns holders once every requested chunk is at the target status.
    #[must_use]
    pub fn ready_chunks(&self) -> Option<ReadyChunks> {
        let inner = self.inner.as_ref()?;
        let mut holders = Vec::with_capacity(inner.positions.len());

        for &pos in &inner.positions {
            let holder = inner
                .chunk_map
                .chunks
                .read_sync(&pos, |_, holder| holder.clone())?;
            {
                let _chunk = holder.try_chunk(inner.status)?;
            }
            holders.push(holder);
        }

        Some(ReadyChunks {
            status: inner.status,
            holders,
        })
    }

    /// Cancels the request and releases its tickets.
    pub fn cancel(&mut self) {
        self.release_tickets();
    }

    fn release_tickets(&mut self) {
        let Some(inner) = self.inner.take() else {
            return;
        };

        let mut tickets = inner.chunk_map.chunk_tickets.lock();
        for pos in inner.positions {
            tickets.remove_ticket(pos, inner.ticket);
        }
    }
}

impl Drop for ChunkRequestHandle {
    fn drop(&mut self) {
        self.release_tickets();
    }
}

impl ChunkMap {
    /// Adds tickets for a chunk request and returns a pollable handle.
    ///
    /// The returned handle owns the tickets. Holder creation and generation
    /// scheduling are handled by the chunk scheduling tick.
    #[must_use]
    pub fn request_chunks(self: &Arc<Self>, request: ChunkRequest) -> ChunkRequestHandle {
        ChunkRequestHandle::new(self.clone(), request)
    }

    /// Requests one chunk at `status`.
    #[must_use]
    pub fn request_chunk(
        self: &Arc<Self>,
        pos: ChunkPos,
        status: ChunkStatus,
        ticket_kind: ChunkTicketKind,
    ) -> ChunkRequestHandle {
        self.request_chunks(ChunkRequest {
            status,
            positions: vec![pos],
            ticket_kind,
        })
    }

    /// Requests a square of chunks centered on `center`.
    #[must_use]
    pub fn request_square(
        self: &Arc<Self>,
        center: ChunkPos,
        radius: u8,
        status: ChunkStatus,
        ticket_kind: ChunkTicketKind,
    ) -> ChunkRequestHandle {
        let radius = i32::from(radius);
        let diameter = radius * 2 + 1;
        let capacity = (diameter * diameter) as usize;
        let mut positions = Vec::with_capacity(capacity);

        for dz in -radius..=radius {
            for dx in -radius..=radius {
                positions.push(ChunkPos::new(center.0.x + dx, center.0.y + dz));
            }
        }

        self.request_chunks(ChunkRequest {
            status,
            positions,
            ticket_kind,
        })
    }
}

fn dedupe_positions(positions: Vec<ChunkPos>) -> Box<[ChunkPos]> {
    let mut seen = FxHashSet::default();
    let mut deduped = Vec::with_capacity(positions.len());
    for pos in positions {
        if seen.insert(pos) {
            deduped.push(pos);
        }
    }
    deduped.into_boxed_slice()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupe_positions_preserves_first_occurrence_order() {
        let positions = dedupe_positions(vec![
            ChunkPos::new(1, 2),
            ChunkPos::new(3, 4),
            ChunkPos::new(1, 2),
        ]);
        assert_eq!(&*positions, &[ChunkPos::new(1, 2), ChunkPos::new(3, 4)]);
    }
}
