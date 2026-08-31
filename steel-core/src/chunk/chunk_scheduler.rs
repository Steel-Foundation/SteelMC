//! Sequencing between gameplay ticket changes and background chunk scheduling.

use std::{
    mem,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use steel_utils::{ChunkPos, locks::SyncMutex};
use uuid::Uuid;

use crate::chunk::gameplay_chunk_lookup_cache::GameplayChunkLookupCacheStats;
use crate::chunk::{
    chunk_ticket_manager::{ChunkTicket, ChunkTicketLevel, LoadLevelChange, LoadTicketManager},
    simulation_ticket_manager::SimulationTicketManager,
};

/// Timing information for one background epoch and its boundary commit.
#[derive(Debug, Default)]
pub struct ChunkMapSchedulingTimings {
    /// Time spent applying queued ticket operations and propagating their levels.
    pub ticket_updates: Duration,
    /// Time spent finalizing block-entity unloads before the boundary commit.
    pub block_entity_unloads: Duration,
    /// Time spent revoking ticking readiness before holder lifecycle changes.
    pub readiness_demotions: Duration,
    /// Time spent committing holder lifecycle changes at the game-tick boundary.
    pub lifecycle_commit: Duration,
    /// Time spent reconciling Full neighborhoods and applying ticking readiness.
    pub readiness_reconcile: Duration,
    /// Subset of `readiness_reconcile` spent running generation post-processing.
    pub post_process_generation: Duration,
    /// Number of chunks whose generation post-processing completed.
    pub post_process_chunk_count: usize,
    /// Number of packed generation post-processing positions attempted.
    pub post_process_position_count: usize,
    /// Number of readiness candidates considered during reconciliation.
    pub readiness_candidate_count: usize,
    /// Time spent rebuilding the published ticking-chunk snapshot.
    pub ticking_snapshot_rebuild: Duration,
    /// Number of block-ticking chunks in a snapshot rebuilt during this epoch.
    pub rebuilt_ticking_chunk_count: usize,
    /// Scoped holder-cache activity during readiness reconciliation.
    pub lookup_cache: GameplayChunkLookupCacheStats,
    /// Time spent creating or updating chunk-generation tasks.
    pub schedule_generation: Duration,
    /// Number of holders scheduled for generation.
    pub scheduled_count: usize,
    /// Time spent refilling generation worker slots.
    pub run_generation: Duration,
    /// Time spent processing physical chunk unloads.
    pub process_unloads: Duration,
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

/// Revision assigned to an ordered batch of load-ticket operations.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LoadTicketRevision(u64);

impl LoadTicketRevision {
    const INITIAL: Self = Self(0);

    fn next(self) -> Self {
        assert_ne!(self.0, u64::MAX, "load ticket revision exhausted");
        Self(self.0 + 1)
    }
}

/// Revision assigned to an ordered batch of simulation-ticket operations.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SimulationTicketRevision(u64);

impl SimulationTicketRevision {
    const INITIAL: Self = Self(0);

    fn next(self) -> Self {
        assert_ne!(self.0, u64::MAX, "simulation ticket revision exhausted");
        Self(self.0 + 1)
    }
}

/// Revisions assigned to the domains affected by one ingress submission.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChunkTicketReceipt {
    load: Option<LoadTicketRevision>,
    simulation: Option<SimulationTicketRevision>,
}

impl ChunkTicketReceipt {
    #[must_use]
    #[cfg(test)]
    pub(crate) const fn load(self) -> Option<LoadTicketRevision> {
        self.load
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) const fn simulation(self) -> Option<SimulationTicketRevision> {
        self.simulation
    }
}

/// One source-level ticket mutation submitted by gameplay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChunkTicketOperation {
    Add {
        pos: ChunkPos,
        ticket: ChunkTicket,
    },
    Remove {
        pos: ChunkPos,
        ticket: ChunkTicket,
    },
    AddPlayer {
        pos: ChunkPos,
        player_id: Uuid,
        load_level: ChunkTicketLevel,
    },
    RemovePlayer {
        pos: ChunkPos,
        player_id: Uuid,
    },
}

impl ChunkTicketOperation {
    const fn load_operation(self) -> LoadTicketOperation {
        match self {
            Self::Add { pos, ticket } => LoadTicketOperation::Add { pos, ticket },
            Self::Remove { pos, ticket } => LoadTicketOperation::Remove { pos, ticket },
            Self::AddPlayer {
                pos,
                player_id,
                load_level,
            } => LoadTicketOperation::AddPlayer {
                pos,
                player_id,
                load_level,
            },
            Self::RemovePlayer { pos, player_id } => {
                LoadTicketOperation::RemovePlayer { pos, player_id }
            }
        }
    }

    const fn simulation_operation(self) -> Option<SimulationTicketOperation> {
        match self {
            Self::Add { pos, ticket } if ticket.simulation_level().is_some() => {
                Some(SimulationTicketOperation::Add { pos, ticket })
            }
            Self::Remove { pos, ticket } if ticket.simulation_level().is_some() => {
                Some(SimulationTicketOperation::Remove { pos, ticket })
            }
            Self::Add { .. } | Self::Remove { .. } => None,
            Self::AddPlayer {
                pos,
                player_id,
                load_level: _,
            } => Some(SimulationTicketOperation::AddPlayer { pos, player_id }),
            Self::RemovePlayer { pos, player_id } => {
                Some(SimulationTicketOperation::RemovePlayer { pos, player_id })
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadTicketOperation {
    Add {
        pos: ChunkPos,
        ticket: ChunkTicket,
    },
    Remove {
        pos: ChunkPos,
        ticket: ChunkTicket,
    },
    AddPlayer {
        pos: ChunkPos,
        player_id: Uuid,
        load_level: ChunkTicketLevel,
    },
    RemovePlayer {
        pos: ChunkPos,
        player_id: Uuid,
    },
}

impl LoadTicketOperation {
    fn apply(self, ticket_manager: &mut LoadTicketManager) {
        match self {
            Self::Add { pos, ticket } => ticket_manager.add_ticket(pos, ticket),
            Self::Remove { pos, ticket } => {
                ticket_manager.remove_ticket(pos, ticket);
            }
            Self::AddPlayer {
                pos,
                player_id,
                load_level,
            } => {
                ticket_manager.add_player(pos, player_id, load_level);
            }
            Self::RemovePlayer { pos, player_id } => {
                ticket_manager.remove_player(pos, player_id);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SimulationTicketOperation {
    Add { pos: ChunkPos, ticket: ChunkTicket },
    Remove { pos: ChunkPos, ticket: ChunkTicket },
    AddPlayer { pos: ChunkPos, player_id: Uuid },
    RemovePlayer { pos: ChunkPos, player_id: Uuid },
}

impl SimulationTicketOperation {
    fn apply(self, ticket_manager: &mut SimulationTicketManager) {
        match self {
            Self::Add { pos, ticket } => {
                ticket_manager.add_ticket(pos, ticket);
            }
            Self::Remove { pos, ticket } => {
                ticket_manager.remove_ticket(pos, ticket);
            }
            Self::AddPlayer { pos, player_id } => {
                ticket_manager.add_player(pos, player_id);
            }
            Self::RemovePlayer { pos, player_id } => {
                ticket_manager.remove_player(pos, player_id);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QueuedTicketOperation<R, O> {
    revision: R,
    operation: O,
}

#[derive(Debug)]
struct PendingTicketOperations<R, O> {
    next_revision: R,
    operations: Vec<QueuedTicketOperation<R, O>>,
    recycled_operations: Vec<QueuedTicketOperation<R, O>>,
}

impl<R: Copy, O> PendingTicketOperations<R, O> {
    const fn new(initial_revision: R) -> Self {
        Self {
            next_revision: initial_revision,
            operations: Vec::new(),
            recycled_operations: Vec::new(),
        }
    }

    fn push(&mut self, revision: R, operation: O) {
        self.operations.push(QueuedTicketOperation {
            revision,
            operation,
        });
    }

    fn take(&mut self) -> Vec<QueuedTicketOperation<R, O>> {
        mem::replace(
            &mut self.operations,
            mem::take(&mut self.recycled_operations),
        )
    }

    fn recycle(&mut self, mut operations: Vec<QueuedTicketOperation<R, O>>) {
        operations.clear();
        self.recycled_operations = operations;
    }
}

type PendingLoadTicketOperations = PendingTicketOperations<LoadTicketRevision, LoadTicketOperation>;
type PendingSimulationTicketOperations =
    PendingTicketOperations<SimulationTicketRevision, SimulationTicketOperation>;

#[derive(Debug)]
struct TicketOperationIngress {
    load: PendingLoadTicketOperations,
    simulation: PendingSimulationTicketOperations,
}

impl Default for TicketOperationIngress {
    fn default() -> Self {
        Self {
            load: PendingTicketOperations::new(LoadTicketRevision::INITIAL),
            simulation: PendingTicketOperations::new(SimulationTicketRevision::INITIAL),
        }
    }
}

impl TicketOperationIngress {
    fn push(&mut self, operation: ChunkTicketOperation) -> ChunkTicketReceipt {
        let mut receipt = ChunkTicketReceipt::default();
        self.push_operation(operation, &mut receipt);
        receipt
    }

    fn push_batch(
        &mut self,
        operations: impl IntoIterator<Item = ChunkTicketOperation>,
    ) -> Option<ChunkTicketReceipt> {
        let mut receipt = None;
        for operation in operations {
            let receipt = receipt.get_or_insert_with(ChunkTicketReceipt::default);
            self.push_operation(operation, receipt);
        }
        receipt
    }

    fn push_operation(
        &mut self,
        operation: ChunkTicketOperation,
        receipt: &mut ChunkTicketReceipt,
    ) {
        let load_operation = operation.load_operation();
        let revision = *receipt.load.get_or_insert_with(|| {
            let revision = self.load.next_revision.next();
            self.load.next_revision = revision;
            revision
        });
        self.load.push(revision, load_operation);

        if let Some(simulation_operation) = operation.simulation_operation() {
            let revision = *receipt.simulation.get_or_insert_with(|| {
                let revision = self.simulation.next_revision.next();
                self.simulation.next_revision = revision;
                revision
            });
            self.simulation.push(revision, simulation_operation);
        }
    }
}

pub(crate) struct PreparedChunkSchedulingEpoch {
    pub ticket_manager: LoadTicketManager,
    pub applied_revision: LoadTicketRevision,
    pub changes: Vec<LoadLevelChange>,
    pub timings: ChunkMapPreparationTimings,
}

enum ChunkSchedulingState {
    Idle {
        ticket_manager: LoadTicketManager,
        applied_revision: LoadTicketRevision,
    },
    Running,
    Ready(PreparedChunkSchedulingEpoch),
}

pub(crate) enum ChunkSchedulingBoundaryStep {
    Running,
    Start {
        ticket_manager: LoadTicketManager,
        applied_revision: LoadTicketRevision,
    },
    Commit(PreparedChunkSchedulingEpoch),
}

/// Owns the short ticket-ingress lock and the non-blocking epoch handoff.
/// The propagation manager moves between epochs instead of being cloned or
/// locked by gameplay.
pub(crate) struct ChunkSchedulingCoordinator {
    ticket_operation_ingress: SyncMutex<TicketOperationIngress>,
    state: SyncMutex<ChunkSchedulingState>,
    committed_load_revision: AtomicU64,
    committed_simulation_revision: AtomicU64,
}

impl ChunkSchedulingCoordinator {
    pub fn new(ticket_manager: LoadTicketManager) -> Self {
        Self {
            ticket_operation_ingress: SyncMutex::new(TicketOperationIngress::default()),
            state: SyncMutex::new(ChunkSchedulingState::Idle {
                ticket_manager,
                applied_revision: LoadTicketRevision::INITIAL,
            }),
            committed_load_revision: AtomicU64::new(LoadTicketRevision::INITIAL.0),
            committed_simulation_revision: AtomicU64::new(SimulationTicketRevision::INITIAL.0),
        }
    }

    pub fn queue_ticket_operation(&self, operation: ChunkTicketOperation) -> ChunkTicketReceipt {
        self.ticket_operation_ingress.lock().push(operation)
    }

    pub fn queue_ticket_operations(
        &self,
        operations: impl IntoIterator<Item = ChunkTicketOperation>,
    ) -> Option<ChunkTicketReceipt> {
        self.ticket_operation_ingress.lock().push_batch(operations)
    }

    pub fn apply_pending_load_ticket_operations(
        &self,
        ticket_manager: &mut LoadTicketManager,
        applied_revision: LoadTicketRevision,
    ) -> LoadTicketRevision {
        let mut operations = self.ticket_operation_ingress.lock().load.take();
        let mut latest_revision = applied_revision;
        for queued in operations.drain(..) {
            queued.operation.apply(ticket_manager);
            latest_revision = queued.revision;
        }
        self.ticket_operation_ingress
            .lock()
            .load
            .recycle(operations);
        latest_revision
    }

    pub fn apply_pending_simulation_ticket_operations(
        &self,
        ticket_manager: &mut SimulationTicketManager,
        applied_revision: SimulationTicketRevision,
    ) -> SimulationTicketRevision {
        let mut operations = self.ticket_operation_ingress.lock().simulation.take();
        let mut latest_revision = applied_revision;
        for queued in operations.drain(..) {
            queued.operation.apply(ticket_manager);
            latest_revision = queued.revision;
        }
        self.ticket_operation_ingress
            .lock()
            .simulation
            .recycle(operations);
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

    pub fn publish_committed_load_revision(&self, revision: LoadTicketRevision) {
        self.committed_load_revision
            .store(revision.0, Ordering::Release);
    }

    pub fn publish_committed_simulation_revision(&self, revision: SimulationTicketRevision) {
        self.committed_simulation_revision
            .store(revision.0, Ordering::Release);
    }

    #[must_use]
    pub fn is_load_revision_committed(&self, revision: LoadTicketRevision) -> bool {
        self.committed_load_revision.load(Ordering::Acquire) >= revision.0
    }

    #[must_use]
    pub fn is_simulation_revision_committed(&self, revision: SimulationTicketRevision) -> bool {
        self.committed_simulation_revision.load(Ordering::Acquire) >= revision.0
    }

    #[must_use]
    pub fn is_receipt_committed(&self, receipt: ChunkTicketReceipt) -> bool {
        receipt
            .load
            .is_none_or(|revision| self.is_load_revision_committed(revision))
            && receipt
                .simulation
                .is_none_or(|revision| self.is_simulation_revision_committed(revision))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    struct MixedBatchFixture {
        first_pos: ChunkPos,
        second_pos: ChunkPos,
        player_id: Uuid,
        player_load_level: ChunkTicketLevel,
        load_only: ChunkTicket,
        simulated: ChunkTicket,
    }

    fn assert_mixed_load_projection(
        operations: &[QueuedTicketOperation<LoadTicketRevision, LoadTicketOperation>],
        revision: LoadTicketRevision,
        fixture: MixedBatchFixture,
    ) {
        assert_eq!(
            operations,
            [
                QueuedTicketOperation {
                    revision,
                    operation: LoadTicketOperation::Add {
                        pos: fixture.first_pos,
                        ticket: fixture.load_only,
                    },
                },
                QueuedTicketOperation {
                    revision,
                    operation: LoadTicketOperation::AddPlayer {
                        pos: fixture.first_pos,
                        player_id: fixture.player_id,
                        load_level: fixture.player_load_level,
                    },
                },
                QueuedTicketOperation {
                    revision,
                    operation: LoadTicketOperation::Add {
                        pos: fixture.second_pos,
                        ticket: fixture.simulated,
                    },
                },
                QueuedTicketOperation {
                    revision,
                    operation: LoadTicketOperation::RemovePlayer {
                        pos: fixture.first_pos,
                        player_id: fixture.player_id,
                    },
                },
                QueuedTicketOperation {
                    revision,
                    operation: LoadTicketOperation::Remove {
                        pos: fixture.second_pos,
                        ticket: fixture.simulated,
                    },
                },
            ]
        );
    }

    fn assert_mixed_simulation_projection(
        operations: &[QueuedTicketOperation<SimulationTicketRevision, SimulationTicketOperation>],
        revision: SimulationTicketRevision,
        fixture: MixedBatchFixture,
    ) {
        assert_eq!(
            operations,
            [
                QueuedTicketOperation {
                    revision,
                    operation: SimulationTicketOperation::AddPlayer {
                        pos: fixture.first_pos,
                        player_id: fixture.player_id,
                    },
                },
                QueuedTicketOperation {
                    revision,
                    operation: SimulationTicketOperation::Add {
                        pos: fixture.second_pos,
                        ticket: fixture.simulated,
                    },
                },
                QueuedTicketOperation {
                    revision,
                    operation: SimulationTicketOperation::RemovePlayer {
                        pos: fixture.first_pos,
                        player_id: fixture.player_id,
                    },
                },
                QueuedTicketOperation {
                    revision,
                    operation: SimulationTicketOperation::Remove {
                        pos: fixture.second_pos,
                        ticket: fixture.simulated,
                    },
                },
            ]
        );
    }

    #[test]
    fn mixed_batch_fans_out_in_source_order() {
        let load_only = ChunkTicket::full_chunks(0);
        let simulated = ChunkTicket::simulated_full_chunks(1);
        let player_load_level = ChunkTicketLevel::ENTITY_TICKING_CHUNK;
        let first_pos = ChunkPos::new(3, -2);
        let second_pos = ChunkPos::new(-4, 5);
        let player_id = Uuid::from_u128(1);
        let fixture = MixedBatchFixture {
            first_pos,
            second_pos,
            player_id,
            player_load_level,
            load_only,
            simulated,
        };
        let mut ingress = TicketOperationIngress::default();

        let receipt = ingress
            .push_batch([
                ChunkTicketOperation::Add {
                    pos: first_pos,
                    ticket: load_only,
                },
                ChunkTicketOperation::AddPlayer {
                    pos: first_pos,
                    player_id,
                    load_level: player_load_level,
                },
                ChunkTicketOperation::Add {
                    pos: second_pos,
                    ticket: simulated,
                },
                ChunkTicketOperation::RemovePlayer {
                    pos: first_pos,
                    player_id,
                },
                ChunkTicketOperation::Remove {
                    pos: second_pos,
                    ticket: simulated,
                },
            ])
            .expect("non-empty batch should produce a receipt");
        let load_revision = receipt.load().expect("batch affects loading");
        let simulation_revision = receipt.simulation().expect("batch affects simulation");

        assert_mixed_load_projection(&ingress.load.operations, load_revision, fixture);
        assert_mixed_simulation_projection(
            &ingress.simulation.operations,
            simulation_revision,
            fixture,
        );
    }

    #[test]
    fn batches_keep_independent_domain_order() {
        let load_only = ChunkTicket::full_chunks(0);
        let simulated = ChunkTicket::simulated_full_chunks(0);
        let pos = ChunkPos::new(3, -2);
        let player_id = Uuid::from_u128(2);
        let player_load_level = ChunkTicketLevel::ENTITY_TICKING_CHUNK;
        let mut ingress = TicketOperationIngress::default();

        let first_load = ingress.push(ChunkTicketOperation::Add {
            pos,
            ticket: load_only,
        });
        let first_player = ingress.push(ChunkTicketOperation::AddPlayer {
            pos,
            player_id,
            load_level: player_load_level,
        });
        let shared = ingress.push(ChunkTicketOperation::Add {
            pos,
            ticket: simulated,
        });

        assert_eq!(first_load.load(), Some(LoadTicketRevision(1)));
        assert_eq!(first_load.simulation(), None);
        assert_eq!(first_player.load(), Some(LoadTicketRevision(2)));
        assert_eq!(first_player.simulation(), Some(SimulationTicketRevision(1)));
        assert_eq!(shared.load(), Some(LoadTicketRevision(3)));
        assert_eq!(shared.simulation(), Some(SimulationTicketRevision(2)));
        assert_eq!(
            ingress
                .load
                .operations
                .iter()
                .map(|queued| queued.revision)
                .collect::<Vec<_>>(),
            [
                LoadTicketRevision(1),
                LoadTicketRevision(2),
                LoadTicketRevision(3)
            ]
        );
        assert_eq!(
            ingress
                .simulation
                .operations
                .iter()
                .map(|queued| queued.revision)
                .collect::<Vec<_>>(),
            [SimulationTicketRevision(1), SimulationTicketRevision(2)]
        );
    }

    #[test]
    fn player_mutations_advance_both_domain_revisions() {
        let coordinator = ChunkSchedulingCoordinator::new(LoadTicketManager::new());
        let pos = ChunkPos::new(0, 0);
        let player_id = Uuid::from_u128(3);
        let load_ticket = ChunkTicket::full_chunks(0);
        let player_load_level = ChunkTicketLevel::ENTITY_TICKING_CHUNK;
        let first_load = coordinator.queue_ticket_operation(ChunkTicketOperation::Add {
            pos,
            ticket: load_ticket,
        });
        let player_add = coordinator.queue_ticket_operation(ChunkTicketOperation::AddPlayer {
            pos,
            player_id,
            load_level: player_load_level,
        });
        let player_remove = coordinator
            .queue_ticket_operation(ChunkTicketOperation::RemovePlayer { pos, player_id });
        let second_load = coordinator.queue_ticket_operation(ChunkTicketOperation::Remove {
            pos,
            ticket: load_ticket,
        });

        assert_eq!(first_load.load(), Some(LoadTicketRevision(1)));
        assert_eq!(player_add.load(), Some(LoadTicketRevision(2)));
        assert_eq!(player_remove.load(), Some(LoadTicketRevision(3)));
        assert_eq!(second_load.load(), Some(LoadTicketRevision(4)));
        assert_eq!(player_add.simulation(), Some(SimulationTicketRevision(1)));
        assert_eq!(
            player_remove.simulation(),
            Some(SimulationTicketRevision(2))
        );
    }

    #[test]
    fn player_identity_semantics_are_delegated_to_each_manager() {
        let coordinator = ChunkSchedulingCoordinator::new(LoadTicketManager::new());
        let old_pos = ChunkPos::new(0, 0);
        let new_pos = ChunkPos::new(100, 0);
        let player_id = Uuid::from_u128(4);
        let load_level = ChunkTicketLevel::ENTITY_TICKING_CHUNK;
        let receipt = coordinator
            .queue_ticket_operations([
                ChunkTicketOperation::AddPlayer {
                    pos: old_pos,
                    player_id,
                    load_level,
                },
                ChunkTicketOperation::AddPlayer {
                    pos: old_pos,
                    player_id,
                    load_level,
                },
                ChunkTicketOperation::AddPlayer {
                    pos: new_pos,
                    player_id,
                    load_level,
                },
                ChunkTicketOperation::RemovePlayer {
                    pos: old_pos,
                    player_id,
                },
            ])
            .expect("non-empty batch should produce a receipt");
        let mut load_manager = LoadTicketManager::new();
        let mut simulation_manager = SimulationTicketManager::new();

        let applied_load = coordinator
            .apply_pending_load_ticket_operations(&mut load_manager, LoadTicketRevision::INITIAL);
        let applied_simulation = coordinator.apply_pending_simulation_ticket_operations(
            &mut simulation_manager,
            SimulationTicketRevision::INITIAL,
        );
        load_manager.run_all_updates();
        simulation_manager.run_all_updates(0);

        assert_eq!(receipt.load(), Some(applied_load));
        assert_eq!(receipt.simulation(), Some(applied_simulation));
        assert_eq!(load_manager.get_level(old_pos), None);
        assert_eq!(load_manager.get_level(new_pos), Some(load_level));
        assert_eq!(simulation_manager.get_level(old_pos), None);
        assert_eq!(
            simulation_manager.get_level(new_pos),
            Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
        );
    }

    #[test]
    fn receipt_commit_checks_only_affected_domains() {
        let coordinator = ChunkSchedulingCoordinator::new(LoadTicketManager::new());
        let pos = ChunkPos::new(0, 0);
        let load_only = coordinator.queue_ticket_operation(ChunkTicketOperation::Add {
            pos,
            ticket: ChunkTicket::full_chunks(0),
        });
        let shared = coordinator.queue_ticket_operation(ChunkTicketOperation::Add {
            pos,
            ticket: ChunkTicket::simulated_full_chunks(0),
        });

        coordinator
            .publish_committed_load_revision(load_only.load().expect("ticket affects loading"));
        assert!(coordinator.is_receipt_committed(load_only));
        assert!(!coordinator.is_receipt_committed(shared));

        coordinator.publish_committed_load_revision(shared.load().expect("ticket affects loading"));
        assert!(!coordinator.is_receipt_committed(shared));

        coordinator.publish_committed_simulation_revision(
            shared.simulation().expect("ticket affects simulation"),
        );
        assert!(coordinator.is_receipt_committed(shared));
    }

    #[test]
    fn prepared_load_revision_is_not_visible_before_boundary_publication() {
        let coordinator = ChunkSchedulingCoordinator::new(LoadTicketManager::new());
        let receipt = coordinator
            .queue_ticket_operations([ChunkTicketOperation::Add {
                pos: ChunkPos::new(0, 0),
                ticket: ChunkTicket::full_chunks(0),
            }])
            .expect("non-empty batch should produce a receipt");
        let revision = receipt.load().expect("ticket affects loading");
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
            coordinator.apply_pending_load_ticket_operations(&mut ticket_manager, applied_revision);
        ticket_manager.run_all_updates();
        let changes = ticket_manager.take_changes();
        coordinator.finish_epoch(PreparedChunkSchedulingEpoch {
            ticket_manager,
            applied_revision: applied,
            changes,
            timings: ChunkMapPreparationTimings::default(),
        });

        assert_eq!(applied, revision);
        assert!(!coordinator.is_load_revision_committed(revision));

        let ChunkSchedulingBoundaryStep::Commit(epoch) = coordinator.take_boundary_step() else {
            panic!("finished epoch should be committed at the next boundary");
        };
        coordinator.publish_committed_load_revision(epoch.applied_revision);

        assert!(coordinator.is_load_revision_committed(revision));
    }
}
