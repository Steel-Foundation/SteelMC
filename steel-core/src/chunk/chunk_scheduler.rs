//! Sequencing between gameplay ticket changes and background chunk scheduling.

use std::{
    mem,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use rustc_hash::FxHashSet;
use steel_utils::{ChunkPos, locks::SyncMutex};

use crate::chunk::gameplay_chunk_lookup_cache::GameplayChunkLookupCacheStats;
use crate::chunk::{
    chunk_ticket_manager::{LoadLevelChange, LoadTicketManager},
    chunk_ticket_storage::{
        ChunkTicketStorage, PersistentChunkTickets, SourceLevelUpdate, SourceProjectionChanges,
        TimedTicketExpiration,
    },
    simulation_ticket_manager::SimulationTicketManager,
};

pub(crate) use crate::chunk::chunk_ticket_storage::ChunkTicketOperation;

/// Timing information for one background epoch and its boundary commit.
#[derive(Debug, Default)]
pub struct ChunkMapSchedulingTimings {
    /// Time spent applying queued source projections and propagating their levels.
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

#[derive(Debug)]
struct PendingSourceProjection<R> {
    latest_revision: R,
    dirty_positions: FxHashSet<ChunkPos>,
}

impl<R: Copy> PendingSourceProjection<R> {
    fn new(initial_revision: R) -> Self {
        Self {
            latest_revision: initial_revision,
            dirty_positions: FxHashSet::default(),
        }
    }

    fn allocate_revision(&mut self, next: impl FnOnce(R) -> R) -> R {
        let revision = next(self.latest_revision);
        self.latest_revision = revision;
        revision
    }

    fn mark_positions(&mut self, positions: impl IntoIterator<Item = ChunkPos>) {
        self.dirty_positions.extend(positions);
    }

    fn take_positions(&mut self) -> Vec<ChunkPos> {
        let mut positions: Vec<_> = mem::take(&mut self.dirty_positions).into_iter().collect();
        positions.sort_unstable_by_key(|pos| (pos.0.x, pos.0.y));
        positions
    }
}

#[derive(Debug)]
struct TicketOperationIngress {
    storage: ChunkTicketStorage,
    load: PendingSourceProjection<LoadTicketRevision>,
    simulation: PendingSourceProjection<SimulationTicketRevision>,
}

impl TicketOperationIngress {
    fn new(storage: ChunkTicketStorage) -> Self {
        Self {
            storage,
            load: PendingSourceProjection::new(LoadTicketRevision::INITIAL),
            simulation: PendingSourceProjection::new(SimulationTicketRevision::INITIAL),
        }
    }

    fn push(&mut self, operation: ChunkTicketOperation) -> ChunkTicketReceipt {
        let changes = self.storage.apply(operation);
        let receipt = self.record_changes(changes);
        assert!(
            receipt.load.is_some(),
            "every submitted ticket operation must affect loading"
        );
        receipt
    }

    fn push_batch(
        &mut self,
        operations: impl IntoIterator<Item = ChunkTicketOperation>,
    ) -> Option<ChunkTicketReceipt> {
        let changes = self.storage.apply_operations(operations);
        if !changes.load_domain_affected {
            return None;
        }

        Some(self.record_changes(changes))
    }

    fn record_changes(&mut self, changes: SourceProjectionChanges) -> ChunkTicketReceipt {
        let SourceProjectionChanges {
            load_positions,
            simulation_positions,
            load_domain_affected,
            simulation_domain_affected,
        } = changes;

        let load = load_domain_affected.then(|| {
            self.load.mark_positions(load_positions);
            self.load.allocate_revision(LoadTicketRevision::next)
        });
        let simulation = simulation_domain_affected.then(|| {
            self.simulation.mark_positions(simulation_positions);
            self.simulation
                .allocate_revision(SimulationTicketRevision::next)
        });

        ChunkTicketReceipt { load, simulation }
    }

    fn take_load_projection(&mut self) -> (Vec<SourceLevelUpdate>, LoadTicketRevision) {
        let positions = self.load.take_positions();
        let updates = positions
            .into_iter()
            .map(|pos| self.storage.load_source_update(pos))
            .collect();
        (updates, self.load.latest_revision)
    }

    fn take_simulation_projection(
        &mut self,
        simulation_distance: u8,
    ) -> (Vec<SourceLevelUpdate>, SimulationTicketRevision) {
        let distance_changes = self.storage.set_simulation_distance(simulation_distance);
        self.simulation
            .mark_positions(distance_changes.simulation_positions);

        let positions = self.simulation.take_positions();
        let updates = positions
            .into_iter()
            .map(|pos| self.storage.simulation_source_update(pos))
            .collect();
        (updates, self.simulation.latest_revision)
    }

    fn record_if_affected(&mut self, changes: SourceProjectionChanges) {
        if !changes.load_domain_affected && !changes.simulation_domain_affected {
            return;
        }

        self.record_changes(changes);
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

/// Owns authoritative ticket sources and the non-blocking epoch handoff.
///
/// Gameplay mutates source ownership under the ingress lock. Loading and
/// simulation propagation consume coalesced level projections outside it.
pub(crate) struct ChunkSchedulingCoordinator {
    ticket_ingress: SyncMutex<TicketOperationIngress>,
    state: SyncMutex<ChunkSchedulingState>,
    committed_load_revision: AtomicU64,
    committed_simulation_revision: AtomicU64,
}

impl ChunkSchedulingCoordinator {
    pub fn new(ticket_storage: ChunkTicketStorage, ticket_manager: LoadTicketManager) -> Self {
        Self {
            ticket_ingress: SyncMutex::new(TicketOperationIngress::new(ticket_storage)),
            state: SyncMutex::new(ChunkSchedulingState::Idle {
                ticket_manager,
                applied_revision: LoadTicketRevision::INITIAL,
            }),
            committed_load_revision: AtomicU64::new(LoadTicketRevision::INITIAL.0),
            committed_simulation_revision: AtomicU64::new(SimulationTicketRevision::INITIAL.0),
        }
    }

    pub fn queue_ticket_operation(&self, operation: ChunkTicketOperation) -> ChunkTicketReceipt {
        self.ticket_ingress.lock().push(operation)
    }

    pub fn queue_ticket_operations(
        &self,
        operations: impl IntoIterator<Item = ChunkTicketOperation>,
    ) -> Option<ChunkTicketReceipt> {
        self.ticket_ingress.lock().push_batch(operations)
    }

    pub fn apply_pending_load_projection(
        &self,
        ticket_manager: &mut LoadTicketManager,
        applied_revision: LoadTicketRevision,
    ) -> LoadTicketRevision {
        let (updates, through_revision) = self.ticket_ingress.lock().take_load_projection();
        assert!(
            through_revision >= applied_revision,
            "load projection revision moved backwards"
        );
        ticket_manager.apply_source_updates(updates);
        through_revision
    }

    pub fn apply_pending_simulation_projection(
        &self,
        ticket_manager: &mut SimulationTicketManager,
        applied_revision: SimulationTicketRevision,
        simulation_distance: u8,
    ) -> SimulationTicketRevision {
        let (updates, through_revision) = self
            .ticket_ingress
            .lock()
            .take_simulation_projection(simulation_distance);
        assert!(
            through_revision >= applied_revision,
            "simulation projection revision moved backwards"
        );
        ticket_manager.apply_source_updates(updates);
        through_revision
    }

    pub(crate) fn add_or_refresh_portal_ticket(&self, pos: ChunkPos) {
        let mut ingress = self.ticket_ingress.lock();
        let changes = ingress.storage.add_or_refresh_portal_ticket(pos);
        ingress.record_if_affected(changes);
    }

    pub(crate) fn add_or_refresh_ender_pearl_ticket(&self, pos: ChunkPos) {
        let mut ingress = self.ticket_ingress.lock();
        let changes = ingress.storage.add_or_refresh_ender_pearl_ticket(pos);
        ingress.record_if_affected(changes);
    }

    #[must_use]
    pub(crate) fn timed_ticket_expirations(&self) -> Vec<TimedTicketExpiration> {
        self.ticket_ingress
            .lock()
            .storage
            .timed_ticket_expirations()
    }

    pub(crate) fn tick_timed_tickets(&self, expirations: &[TimedTicketExpiration]) {
        let mut ingress = self.ticket_ingress.lock();
        let changes = ingress.storage.tick_timed_tickets(expirations);
        ingress.record_if_affected(changes);
    }

    #[must_use]
    pub(crate) fn persistent_chunk_tickets(&self) -> PersistentChunkTickets {
        self.ticket_ingress.lock().storage.to_persistent()
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
    use crate::chunk::chunk_ticket_manager::{ChunkTicket, ChunkTicketLevel};
    use uuid::Uuid;

    fn coordinator(simulation_distance: u8) -> ChunkSchedulingCoordinator {
        ChunkSchedulingCoordinator::new(
            ChunkTicketStorage::new(simulation_distance),
            LoadTicketManager::new(),
        )
    }

    #[test]
    fn operations_coalesce_to_one_final_projection_per_position() {
        let pos = ChunkPos::new(3, -2);
        let weaker = ChunkTicket::simulated_full_chunks(3);
        let stronger = ChunkTicket::simulated_full_chunks(1);
        let mut ingress = TicketOperationIngress::new(ChunkTicketStorage::new(4));

        let receipt = ingress
            .push_batch([
                ChunkTicketOperation::Add {
                    pos,
                    ticket: weaker,
                },
                ChunkTicketOperation::Add {
                    pos,
                    ticket: stronger,
                },
                ChunkTicketOperation::Remove {
                    pos,
                    ticket: weaker,
                },
            ])
            .expect("non-empty batch should produce a receipt");
        let (load_updates, load_revision) = ingress.take_load_projection();
        let (simulation_updates, simulation_revision) = ingress.take_simulation_projection(4);

        assert_eq!(receipt.load(), Some(load_revision));
        assert_eq!(receipt.simulation(), Some(simulation_revision));
        assert_eq!(
            load_updates,
            [SourceLevelUpdate {
                pos,
                level: Some(stronger.load_level()),
            }]
        );
        assert_eq!(
            simulation_updates,
            [SourceLevelUpdate {
                pos,
                level: stronger.simulation_level(),
            }]
        );
    }

    #[test]
    fn no_op_receipts_advance_when_both_projections_are_drained() {
        let coordinator = coordinator(4);
        let pos = ChunkPos::new(0, 0);
        let receipt = coordinator.queue_ticket_operation(ChunkTicketOperation::Remove {
            pos,
            ticket: ChunkTicket::simulated_full_chunks(0),
        });
        let mut load_manager = LoadTicketManager::new();
        let mut simulation_manager = SimulationTicketManager::new();

        let load_revision = coordinator
            .apply_pending_load_projection(&mut load_manager, LoadTicketRevision::INITIAL);
        let simulation_revision = coordinator.apply_pending_simulation_projection(
            &mut simulation_manager,
            SimulationTicketRevision::INITIAL,
            4,
        );

        assert_eq!(receipt.load(), Some(load_revision));
        assert_eq!(receipt.simulation(), Some(simulation_revision));
        assert_eq!(load_manager.run_all_updates(), []);
        assert_eq!(simulation_manager.run_all_updates(), []);
        assert!(!coordinator.is_receipt_committed(receipt));

        coordinator.publish_committed_load_revision(load_revision);
        coordinator.publish_committed_simulation_revision(simulation_revision);
        assert!(coordinator.is_receipt_committed(receipt));
    }

    #[test]
    fn domains_converge_when_a_submission_lands_between_projection_drains() {
        let coordinator = coordinator(4);
        let pos = ChunkPos::new(0, 0);
        let ticket = ChunkTicket::simulated_full_chunks(0);
        let _ = coordinator.queue_ticket_operation(ChunkTicketOperation::Add { pos, ticket });
        let mut load_manager = LoadTicketManager::new();
        let mut simulation_manager = SimulationTicketManager::new();

        let first_load = coordinator
            .apply_pending_load_projection(&mut load_manager, LoadTicketRevision::INITIAL);
        load_manager.run_all_updates();
        assert!(load_manager.get_level(pos).is_some());

        let removal =
            coordinator.queue_ticket_operation(ChunkTicketOperation::Remove { pos, ticket });
        let simulation = coordinator.apply_pending_simulation_projection(
            &mut simulation_manager,
            SimulationTicketRevision::INITIAL,
            4,
        );
        let load = coordinator.apply_pending_load_projection(&mut load_manager, first_load);
        simulation_manager.run_all_updates();
        load_manager.run_all_updates();

        assert_eq!(removal.load(), Some(load));
        assert_eq!(removal.simulation(), Some(simulation));
        assert_eq!(load_manager.get_level(pos), None);
        assert_eq!(simulation_manager.get_level(pos), None);
    }

    #[test]
    fn player_moves_and_stale_removals_use_one_authoritative_source() {
        let simulation_distance = 4;
        let coordinator = coordinator(simulation_distance);
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
            .apply_pending_load_projection(&mut load_manager, LoadTicketRevision::INITIAL);
        let applied_simulation = coordinator.apply_pending_simulation_projection(
            &mut simulation_manager,
            SimulationTicketRevision::INITIAL,
            simulation_distance,
        );
        load_manager.run_all_updates();
        simulation_manager.run_all_updates();

        assert_eq!(receipt.load(), Some(applied_load));
        assert_eq!(receipt.simulation(), Some(applied_simulation));
        assert_eq!(load_manager.get_level(old_pos), None);
        assert_eq!(load_manager.get_level(new_pos), Some(load_level));
        assert_eq!(simulation_manager.get_level(old_pos), None);
        assert_eq!(
            simulation_manager.get_level(new_pos),
            Some(ChunkTicketLevel::for_entity_ticking_radius(
                simulation_distance
            ))
        );
    }

    #[test]
    fn load_and_simulation_domains_commit_independently() {
        let coordinator = coordinator(4);
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
    fn simulation_distance_change_reprojects_players_without_a_new_revision() {
        let initial_distance = 2;
        let updated_distance = 6;
        let coordinator = coordinator(initial_distance);
        let pos = ChunkPos::new(5, -3);
        let receipt = coordinator.queue_ticket_operation(ChunkTicketOperation::AddPlayer {
            pos,
            player_id: Uuid::from_u128(5),
            load_level: ChunkTicketLevel::ENTITY_TICKING_CHUNK,
        });
        let mut simulation_manager = SimulationTicketManager::new();
        let applied_revision = coordinator.apply_pending_simulation_projection(
            &mut simulation_manager,
            SimulationTicketRevision::INITIAL,
            initial_distance,
        );
        simulation_manager.run_all_updates();
        assert_eq!(receipt.simulation(), Some(applied_revision));
        assert_eq!(
            simulation_manager.get_level(pos),
            Some(ChunkTicketLevel::for_entity_ticking_radius(
                initial_distance
            ))
        );

        let unchanged_revision = coordinator.apply_pending_simulation_projection(
            &mut simulation_manager,
            applied_revision,
            updated_distance,
        );
        simulation_manager.run_all_updates();

        assert_eq!(unchanged_revision, applied_revision);
        assert_eq!(
            simulation_manager.get_level(pos),
            Some(ChunkTicketLevel::for_entity_ticking_radius(
                updated_distance
            ))
        );
    }

    #[test]
    fn prepared_load_revision_is_not_visible_before_boundary_publication() {
        let coordinator = coordinator(4);
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
            coordinator.apply_pending_load_projection(&mut ticket_manager, applied_revision);
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
