//! Sequencing between gameplay ticket changes and background chunk scheduling.

use std::{
    mem,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use steel_utils::{ChunkPos, locks::SyncMutex};

use crate::chunk::chunk_ticket_manager::{ChunkTicket, ChunkTicketManager, LevelChange};
use crate::chunk::gameplay_chunk_lookup_cache::GameplayChunkLookupCacheStats;

/// Timing information for one background epoch and its boundary commit.
#[derive(Debug, Default)]
pub(crate) struct ChunkMapSchedulingTimings {
    /// Time spent applying queued ticket operations and propagating their levels.
    pub(crate) ticket_updates: Duration,
    /// Time spent finalizing block-entity unloads before the boundary commit.
    pub(crate) block_entity_unloads: Duration,
    /// Time spent revoking ticking readiness before holder lifecycle changes.
    pub(crate) readiness_demotions: Duration,
    /// Time spent committing holder lifecycle changes at the game-tick boundary.
    pub(crate) lifecycle_commit: Duration,
    /// Time spent reconciling Full neighborhoods and applying ticking readiness.
    pub(crate) readiness_reconcile: Duration,
    /// Subset of `readiness_reconcile` spent running generation post-processing.
    pub(crate) post_process_generation: Duration,
    /// Number of chunks whose generation post-processing completed.
    pub(crate) post_process_chunk_count: usize,
    /// Number of packed generation post-processing positions attempted.
    pub(crate) post_process_position_count: usize,
    /// Number of readiness candidates considered during reconciliation.
    pub(crate) readiness_candidate_count: usize,
    /// Time spent rebuilding the published ticking-chunk snapshot.
    pub(crate) ticking_snapshot_rebuild: Duration,
    /// Number of block-ticking chunks in a snapshot rebuilt during this epoch.
    pub(crate) rebuilt_ticking_chunk_count: usize,
    /// Scoped holder-cache activity during readiness reconciliation.
    pub(crate) lookup_cache: GameplayChunkLookupCacheStats,
    /// Time spent creating or updating chunk-generation tasks.
    pub(crate) schedule_generation: Duration,
    /// Number of holders scheduled for generation.
    pub(crate) scheduled_count: usize,
    /// Time spent refilling generation worker slots.
    pub(crate) run_generation: Duration,
    /// Time spent processing physical chunk unloads.
    pub(crate) process_unloads: Duration,
}

/// Timing information produced by the background half of a scheduling epoch.
///
/// Boundary-only fields stay out of `PreparedChunkSchedulingEpoch` so the
/// cross-thread scheduling state does not grow with game-thread observability.
#[derive(Debug, Default)]
pub(crate) struct ChunkMapPreparationTimings {
    pub(crate) ticket_updates: Duration,
    pub(crate) schedule_generation: Duration,
    pub(crate) scheduled_count: usize,
    pub(crate) run_generation: Duration,
    pub(crate) process_unloads: Duration,
}

impl ChunkMapPreparationTimings {
    pub(crate) fn into_scheduling_timings(self) -> ChunkMapSchedulingTimings {
        ChunkMapSchedulingTimings {
            ticket_updates: self.ticket_updates,
            schedule_generation: self.schedule_generation,
            scheduled_count: self.scheduled_count,
            run_generation: self.run_generation,
            process_unloads: self.process_unloads,
            ..ChunkMapSchedulingTimings::default()
        }
    }
}

/// Revision assigned to an ordered batch of ticket operations.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ChunkTicketRevision(u64);

impl ChunkTicketRevision {
    const INITIAL: Self = Self(0);

    fn next(self) -> Self {
        assert_ne!(self.0, u64::MAX, "chunk ticket revision exhausted");
        Self(self.0 + 1)
    }
}

/// One source-level ticket mutation submitted by gameplay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChunkTicketOperation {
    Add { pos: ChunkPos, ticket: ChunkTicket },
    Remove { pos: ChunkPos, ticket: ChunkTicket },
}

impl ChunkTicketOperation {
    fn apply(self, ticket_manager: &mut ChunkTicketManager) {
        match self {
            Self::Add { pos, ticket } => ticket_manager.add_ticket(pos, ticket),
            Self::Remove { pos, ticket } => {
                ticket_manager.remove_ticket(pos, ticket);
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct QueuedChunkTicketOperation {
    revision: ChunkTicketRevision,
    operation: ChunkTicketOperation,
}

#[derive(Debug)]
struct PendingChunkTicketOperations {
    next_revision: ChunkTicketRevision,
    operations: Vec<QueuedChunkTicketOperation>,
    recycled_operations: Vec<QueuedChunkTicketOperation>,
}

impl Default for PendingChunkTicketOperations {
    fn default() -> Self {
        Self {
            next_revision: ChunkTicketRevision::INITIAL,
            operations: Vec::new(),
            recycled_operations: Vec::new(),
        }
    }
}

impl PendingChunkTicketOperations {
    fn push(&mut self, operation: ChunkTicketOperation) -> ChunkTicketRevision {
        let revision = self.next_revision.next();
        self.next_revision = revision;
        self.operations.push(QueuedChunkTicketOperation {
            revision,
            operation,
        });
        revision
    }

    fn push_batch(
        &mut self,
        operations: impl IntoIterator<Item = ChunkTicketOperation>,
    ) -> Option<ChunkTicketRevision> {
        let mut operations = operations.into_iter();
        let first = operations.next()?;
        let revision = self.next_revision.next();
        self.next_revision = revision;
        self.operations.push(QueuedChunkTicketOperation {
            revision,
            operation: first,
        });
        self.operations
            .extend(operations.map(|operation| QueuedChunkTicketOperation {
                revision,
                operation,
            }));
        Some(revision)
    }

    fn take(&mut self) -> Vec<QueuedChunkTicketOperation> {
        mem::replace(
            &mut self.operations,
            mem::take(&mut self.recycled_operations),
        )
    }

    fn recycle(&mut self, mut operations: Vec<QueuedChunkTicketOperation>) {
        operations.clear();
        self.recycled_operations = operations;
    }
}

pub(crate) struct PreparedChunkSchedulingEpoch {
    pub ticket_manager: ChunkTicketManager,
    pub applied_revision: ChunkTicketRevision,
    pub changes: Vec<LevelChange>,
    pub timings: ChunkMapPreparationTimings,
}

enum ChunkSchedulingState {
    Idle {
        ticket_manager: ChunkTicketManager,
        applied_revision: ChunkTicketRevision,
    },
    Running,
    Ready(PreparedChunkSchedulingEpoch),
}

pub(crate) enum ChunkSchedulingBoundaryStep {
    Running,
    Start {
        ticket_manager: ChunkTicketManager,
        applied_revision: ChunkTicketRevision,
    },
    Commit(PreparedChunkSchedulingEpoch),
}

/// Owns the short ticket-ingress lock and the non-blocking epoch handoff.
/// The propagation manager moves between epochs instead of being cloned or
/// locked by gameplay.
pub(crate) struct ChunkSchedulingCoordinator {
    pending_ticket_operations: SyncMutex<PendingChunkTicketOperations>,
    state: SyncMutex<ChunkSchedulingState>,
    committed_revision: AtomicU64,
}

impl ChunkSchedulingCoordinator {
    pub fn new(ticket_manager: ChunkTicketManager) -> Self {
        Self {
            pending_ticket_operations: SyncMutex::new(PendingChunkTicketOperations::default()),
            state: SyncMutex::new(ChunkSchedulingState::Idle {
                ticket_manager,
                applied_revision: ChunkTicketRevision::INITIAL,
            }),
            committed_revision: AtomicU64::new(ChunkTicketRevision::INITIAL.0),
        }
    }

    pub fn queue_ticket_operation(&self, operation: ChunkTicketOperation) -> ChunkTicketRevision {
        self.pending_ticket_operations.lock().push(operation)
    }

    pub fn queue_ticket_operations(
        &self,
        operations: impl IntoIterator<Item = ChunkTicketOperation>,
    ) -> Option<ChunkTicketRevision> {
        self.pending_ticket_operations.lock().push_batch(operations)
    }

    pub fn apply_pending_ticket_operations(
        &self,
        ticket_manager: &mut ChunkTicketManager,
        applied_revision: ChunkTicketRevision,
    ) -> ChunkTicketRevision {
        let mut operations = self.pending_ticket_operations.lock().take();
        let mut latest_revision = applied_revision;
        for queued in operations.drain(..) {
            queued.operation.apply(ticket_manager);
            latest_revision = queued.revision;
        }
        self.pending_ticket_operations.lock().recycle(operations);
        latest_revision
    }

    pub fn take_boundary_step(&self) -> ChunkSchedulingBoundaryStep {
        let mut state = self.state.lock();
        match mem::replace(&mut *state, ChunkSchedulingState::Running) {
            ChunkSchedulingState::Idle {
                ticket_manager,
                applied_revision,
            } => ChunkSchedulingBoundaryStep::Start {
                ticket_manager,
                applied_revision,
            },
            ChunkSchedulingState::Running => ChunkSchedulingBoundaryStep::Running,
            ChunkSchedulingState::Ready(epoch) => ChunkSchedulingBoundaryStep::Commit(epoch),
        }
    }

    pub fn finish_epoch(&self, epoch: PreparedChunkSchedulingEpoch) {
        let mut state = self.state.lock();
        assert!(
            matches!(*state, ChunkSchedulingState::Running),
            "chunk scheduling epoch finished while another epoch was not running"
        );
        *state = ChunkSchedulingState::Ready(epoch);
    }

    pub fn publish_committed_revision(&self, revision: ChunkTicketRevision) {
        self.committed_revision.store(revision.0, Ordering::Release);
    }

    #[must_use]
    pub fn is_revision_committed(&self, revision: ChunkTicketRevision) -> bool {
        self.committed_revision.load(Ordering::Acquire) >= revision.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_batches_apply_in_submission_order() {
        let coordinator = ChunkSchedulingCoordinator::new(ChunkTicketManager::new());
        let pos = ChunkPos::new(3, -2);
        let ticket = ChunkTicket::full_chunks(0);
        let add_revision = coordinator
            .queue_ticket_operations([ChunkTicketOperation::Add { pos, ticket }])
            .expect("non-empty batch should receive a revision");
        let remove_revision = coordinator
            .queue_ticket_operations([ChunkTicketOperation::Remove { pos, ticket }])
            .expect("non-empty batch should receive a revision");
        let mut manager = ChunkTicketManager::new();

        let applied_revision =
            coordinator.apply_pending_ticket_operations(&mut manager, ChunkTicketRevision::INITIAL);

        assert!(add_revision < remove_revision);
        assert_eq!(applied_revision, remove_revision);
        assert_eq!(manager.ticket_count(), 0);

        let next_revision =
            coordinator.queue_ticket_operation(ChunkTicketOperation::Add { pos, ticket });
        let applied_revision =
            coordinator.apply_pending_ticket_operations(&mut manager, applied_revision);

        assert_eq!(applied_revision, next_revision);
        assert_eq!(manager.ticket_count(), 1);
    }

    #[test]
    fn prepared_revision_is_not_visible_before_boundary_publication() {
        let coordinator = ChunkSchedulingCoordinator::new(ChunkTicketManager::new());
        let revision = coordinator
            .queue_ticket_operations([ChunkTicketOperation::Add {
                pos: ChunkPos::new(0, 0),
                ticket: ChunkTicket::full_chunks(0),
            }])
            .expect("non-empty batch should receive a revision");
        let ChunkSchedulingBoundaryStep::Start {
            mut ticket_manager,
            applied_revision,
        } = coordinator.take_boundary_step()
        else {
            panic!("idle coordinator should start its first epoch");
        };
        assert!(matches!(
            coordinator.take_boundary_step(),
            ChunkSchedulingBoundaryStep::Running
        ));
        let applied =
            coordinator.apply_pending_ticket_operations(&mut ticket_manager, applied_revision);
        ticket_manager.run_all_updates();
        let changes = ticket_manager.take_changes();
        coordinator.finish_epoch(PreparedChunkSchedulingEpoch {
            ticket_manager,
            applied_revision: applied,
            changes,
            timings: ChunkMapPreparationTimings::default(),
        });

        assert_eq!(applied, revision);
        assert!(!coordinator.is_revision_committed(revision));

        let ChunkSchedulingBoundaryStep::Commit(epoch) = coordinator.take_boundary_step() else {
            panic!("finished epoch should be committed at the next boundary");
        };
        coordinator.publish_committed_revision(epoch.applied_revision);

        assert!(coordinator.is_revision_committed(revision));
    }
}
