//! Tick-polled structure location shared by commands and gameplay systems.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use steel_utils::{BlockPos, Identifier};

use crate::{
    chunk::{
        chunk_request::{ChunkRequest, ChunkRequestHandle, ChunkRequestState, ChunkTicketKind},
        status::ChunkStatus,
    },
    world::World,
    worldgen::structure::{StructureLocateCandidate, StructureLocatePlan, squared_distance},
};

/// A located generated structure.
pub(crate) struct LocatedStructure {
    pub(crate) pos: BlockPos,
    pub(crate) structure: Identifier,
    distance_sqr: i64,
}

/// Result of polling a structure location search.
pub(crate) enum StructureLocatePoll {
    Pending,
    Ready(Option<LocatedStructure>),
    Cancelled,
}

enum LocatePhase {
    Start,
    WaitingRings,
    RandomSpread,
    WaitingRandomSpread,
}

/// Detached state machine for Vanilla's generated-structure search.
pub(crate) struct StructureLocator {
    world: Arc<World>,
    plan: StructureLocatePlan,
    origin: BlockPos,
    max_random_radius: i32,
    create_reference: bool,
    phase: LocatePhase,
    pending: Option<ChunkRequestHandle>,
    candidates: Vec<StructureLocateCandidate>,
    best: Option<LocatedStructure>,
    random_radius: i32,
}

impl StructureLocator {
    pub(crate) const fn new(
        world: Arc<World>,
        plan: StructureLocatePlan,
        origin: BlockPos,
        max_random_radius: i32,
        create_reference: bool,
    ) -> Self {
        Self {
            world,
            plan,
            origin,
            max_random_radius,
            create_reference,
            phase: LocatePhase::Start,
            pending: None,
            candidates: Vec::new(),
            best: None,
            random_radius: 0,
        }
    }

    pub(crate) fn poll(&mut self) -> StructureLocatePoll {
        loop {
            match self.phase {
                LocatePhase::Start => {
                    self.candidates = self.plan.ring_candidates(self.origin);
                    if self.candidates.is_empty() {
                        self.phase = LocatePhase::RandomSpread;
                        continue;
                    }
                    self.pending = Some(self.request_current_candidates());
                    self.phase = LocatePhase::WaitingRings;
                    return StructureLocatePoll::Pending;
                }
                LocatePhase::WaitingRings => match self.poll_pending_request() {
                    PendingRequest::Pending => return StructureLocatePoll::Pending,
                    PendingRequest::Cancelled => return StructureLocatePoll::Cancelled,
                    PendingRequest::Ready => {
                        self.update_best_after_rings();
                        self.clear_request();
                        if self.best.is_some() && !self.plan.has_random_spread() {
                            return StructureLocatePoll::Ready(self.best.take());
                        }
                        self.phase = LocatePhase::RandomSpread;
                    }
                },
                LocatePhase::RandomSpread => {
                    if self.random_radius > self.max_random_radius {
                        return StructureLocatePoll::Ready(self.best.take());
                    }
                    self.candidates = self
                        .plan
                        .random_spread_candidates_at_radius(self.origin, self.random_radius);
                    self.random_radius += 1;
                    if self.candidates.is_empty() {
                        continue;
                    }
                    self.pending = Some(self.request_current_candidates());
                    self.phase = LocatePhase::WaitingRandomSpread;
                    return StructureLocatePoll::Pending;
                }
                LocatePhase::WaitingRandomSpread => match self.poll_pending_request() {
                    PendingRequest::Pending => return StructureLocatePoll::Pending,
                    PendingRequest::Cancelled => return StructureLocatePoll::Cancelled,
                    PendingRequest::Ready => {
                        if self.update_best_after_random_radius() {
                            return StructureLocatePoll::Ready(self.best.take());
                        }
                        self.clear_request();
                        self.phase = LocatePhase::RandomSpread;
                    }
                },
            }
        }
    }

    pub(crate) fn cancel(&mut self) {
        if let Some(pending) = &mut self.pending {
            pending.cancel();
        }
    }

    fn request_current_candidates(&self) -> ChunkRequestHandle {
        let positions = self
            .candidates
            .iter()
            .map(|candidate| candidate.chunk_pos)
            .collect();
        self.world.chunk_map.request_chunks(ChunkRequest {
            status: ChunkStatus::StructureStarts,
            positions,
            ticket_kind: ChunkTicketKind::StructureLocate,
        })
    }

    fn poll_pending_request(&self) -> PendingRequest {
        let Some(pending) = &self.pending else {
            return PendingRequest::Cancelled;
        };
        match pending.poll() {
            ChunkRequestState::Pending { .. } => PendingRequest::Pending,
            ChunkRequestState::Ready => PendingRequest::Ready,
            ChunkRequestState::Cancelled => PendingRequest::Cancelled,
        }
    }

    fn clear_request(&mut self) {
        self.pending = None;
        self.candidates.clear();
    }

    fn update_best_after_rings(&mut self) {
        let mut found_scans = FxHashSet::default();
        let mut best = self.best.take();
        for candidate in self.candidates.iter().copied() {
            if found_scans.contains(&candidate.scan_id()) {
                continue;
            }
            let Some(structure) = self.generated_structure_at_candidate(candidate) else {
                continue;
            };
            found_scans.insert(candidate.scan_id());
            let located = LocatedStructure {
                pos: candidate.locate_pos,
                structure,
                distance_sqr: squared_distance(candidate.locate_pos, self.origin),
            };
            if best
                .as_ref()
                .is_none_or(|current| located.distance_sqr < current.distance_sqr)
            {
                best = Some(located);
            }
        }
        self.best = best;
    }

    fn update_best_after_random_radius(&mut self) -> bool {
        let mut best = self.best.take();
        let mut current_scan = None;
        let mut found_current_scan = false;
        let mut found_in_this_radius = false;

        for candidate in self.candidates.iter().copied() {
            if current_scan != Some(candidate.scan_id()) {
                current_scan = Some(candidate.scan_id());
                found_current_scan = false;
            }
            if found_current_scan {
                continue;
            }
            let Some(structure) = self.generated_structure_at_candidate(candidate) else {
                continue;
            };
            found_current_scan = true;
            found_in_this_radius = true;
            let located = LocatedStructure {
                pos: candidate.locate_pos,
                structure,
                distance_sqr: squared_distance(candidate.locate_pos, self.origin),
            };
            if best
                .as_ref()
                .is_none_or(|current| located.distance_sqr < current.distance_sqr)
            {
                best = Some(located);
            }
        }
        self.best = best;
        found_in_this_radius
    }

    fn generated_structure_at_candidate(
        &self,
        candidate: StructureLocateCandidate,
    ) -> Option<Identifier> {
        let holder = self
            .world
            .chunk_map
            .chunks
            .read_sync(&candidate.chunk_pos, |_, holder| Arc::clone(holder))?;
        let chunk = holder.try_chunk(ChunkStatus::StructureStarts)?;
        let structures = self.plan.structures_for_candidate(candidate)?;

        if !self.create_reference {
            let starts = chunk.structure_starts();
            return structures.iter().find_map(|structure| {
                starts
                    .get(structure)
                    .is_some_and(|start| !start.pieces.is_empty())
                    .then(|| structure.clone())
            });
        }

        let found = {
            let mut starts = chunk.structure_starts_mut();
            structures.iter().find_map(|structure| {
                let start = starts.get_mut(structure)?;
                if start.pieces.is_empty() || start.references >= 1 {
                    return None;
                }
                start.references += 1;
                Some(structure.clone())
            })
        };
        if found.is_some() {
            chunk.mark_dirty();
        }
        found
    }
}

enum PendingRequest {
    Pending,
    Ready,
    Cancelled,
}
