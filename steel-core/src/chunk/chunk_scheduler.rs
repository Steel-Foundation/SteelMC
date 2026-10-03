//! Coordinates authoritative ticket storage and chunk-level propagation.

use std::{
    mem,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use rustc_hash::{FxHashMap, FxHashSet};
use steel_registry::steel_ticket_types::CHUNK_REQUEST;
use steel_utils::{ChunkPos, locks::SyncMutex};
use uuid::Uuid;

use crate::chunk::gameplay_chunk_lookup_cache::GameplayChunkLookupCacheStats;
use crate::chunk::{
    chunk_ticket::ChunkTicket,
    chunk_ticket_manager::{ChunkTicketLevel, LoadLevelChange, LoadTicketManager},
    chunk_ticket_storage::{
        ChunkTicketStorage, PersistentChunkTickets, SourceLevelUpdate, SourceProjectionChanges,
        TimedTicketExpiration,
    },
    player_ticket_tracker::PlayerTicketTracker,
    simulation_ticket_manager::{SimulationLevelChange, SimulationTicketManager},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlayerTicketOperation {
    Add { pos: ChunkPos, player_id: Uuid },
    Remove { pos: ChunkPos, player_id: Uuid },
}

/// Timing information for one chunk-source update and its lifecycle commit.
#[derive(Debug, Default)]
pub struct ChunkMapSchedulingTimings {
    /// Time spent applying queued source projections and propagating their levels.
    pub ticket_updates: Duration,
    /// Time spent finalizing block-entity unloads before lifecycle updates.
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
    /// Number of block-ticking chunks in a snapshot rebuilt during this phase.
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

/// Barrier assigned to one ordered, non-empty ticket-operation submission.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ChunkTicketReceipt(u64);

impl ChunkTicketReceipt {
    const INITIAL: Self = Self(0);

    fn next(self) -> Self {
        assert_ne!(self.0, u64::MAX, "chunk ticket receipt exhausted");
        Self(self.0 + 1)
    }
}

/// Coherent loading and simulation changes through one ingress barrier.
#[must_use = "chunk scheduling changes must be applied before publishing the receipt"]
#[derive(Debug, Default)]
pub(crate) struct ChunkSchedulingUpdateBatch {
    pub(crate) through_receipt: ChunkTicketReceipt,
    pub(crate) load_changes: Vec<LoadLevelChange>,
    pub(crate) simulation_changes: Vec<SimulationLevelChange>,
}

#[derive(Debug)]
struct SourceProjectionBatch {
    through_receipt: ChunkTicketReceipt,
    load_updates: Vec<SourceLevelUpdate>,
    simulation_updates: Vec<SourceLevelUpdate>,
}

#[derive(Debug)]
struct TicketOperationIngress {
    storage: ChunkTicketStorage,
    player_tickets: PlayerTicketTracker,
    chunk_request_leases: FxHashMap<(ChunkPos, ChunkTicketLevel), usize>,
    latest_receipt: ChunkTicketReceipt,
    dirty_load_positions: FxHashSet<ChunkPos>,
    dirty_simulation_positions: FxHashSet<ChunkPos>,
}

impl TicketOperationIngress {
    fn new(storage: ChunkTicketStorage, view_distance: u8, simulation_distance: u8) -> Self {
        Self {
            storage,
            player_tickets: PlayerTicketTracker::new(view_distance, simulation_distance),
            chunk_request_leases: FxHashMap::default(),
            latest_receipt: ChunkTicketReceipt::INITIAL,
            dirty_load_positions: FxHashSet::default(),
            dirty_simulation_positions: FxHashSet::default(),
        }
    }

    fn push_player(&mut self, operation: PlayerTicketOperation) -> ChunkTicketReceipt {
        let changes = self.apply_player_operation(operation);
        self.record_changes(changes);
        self.allocate_receipt()
    }

    fn push_player_batch(
        &mut self,
        operations: impl IntoIterator<Item = PlayerTicketOperation>,
    ) -> Option<ChunkTicketReceipt> {
        let mut operations = operations.into_iter().peekable();
        operations.peek()?;
        for operation in operations {
            let changes = self.apply_player_operation(operation);
            self.record_changes(changes);
        }
        Some(self.allocate_receipt())
    }

    fn apply_player_operation(
        &mut self,
        operation: PlayerTicketOperation,
    ) -> SourceProjectionChanges {
        match operation {
            PlayerTicketOperation::Add { pos, player_id } => {
                self.player_tickets
                    .add_player(&mut self.storage, pos, player_id)
            }
            PlayerTicketOperation::Remove { pos, player_id } => {
                self.player_tickets
                    .remove_player(&mut self.storage, pos, player_id)
            }
        }
    }

    fn acquire_chunk_request_leases(
        &mut self,
        positions: impl IntoIterator<Item = ChunkPos>,
        ticket_level: ChunkTicketLevel,
    ) -> Option<ChunkTicketReceipt> {
        let mut positions = positions.into_iter().peekable();
        positions.peek()?;

        for pos in positions {
            let lease_count = self
                .chunk_request_leases
                .entry((pos, ticket_level))
                .or_default();
            assert_ne!(
                *lease_count,
                usize::MAX,
                "chunk request lease count exhausted"
            );
            let needs_ticket = *lease_count == 0;
            *lease_count += 1;

            if needs_ticket {
                let ticket = ChunkTicket::new(&CHUNK_REQUEST, ticket_level);
                let changes = self.storage.add_ticket(pos, ticket);
                self.record_changes(changes);
            }
        }

        Some(self.allocate_receipt())
    }

    fn release_chunk_request_leases(
        &mut self,
        positions: impl IntoIterator<Item = ChunkPos>,
        ticket_level: ChunkTicketLevel,
    ) -> Option<ChunkTicketReceipt> {
        let mut positions = positions.into_iter().peekable();
        positions.peek()?;

        for pos in positions {
            let key = (pos, ticket_level);
            let Some(lease_count) = self.chunk_request_leases.get_mut(&key) else {
                panic!("released an unowned chunk request lease at {pos:?}");
            };
            let remove_ticket = *lease_count == 1;
            if remove_ticket {
                self.chunk_request_leases.remove(&key);
                let ticket = ChunkTicket::new(&CHUNK_REQUEST, ticket_level);
                let changes = self.storage.remove_ticket(pos, ticket);
                self.record_changes(changes);
            } else {
                *lease_count -= 1;
            }
        }

        Some(self.allocate_receipt())
    }

    fn record_changes(&mut self, changes: SourceProjectionChanges) {
        let SourceProjectionChanges {
            load_positions,
            simulation_positions,
        } = changes;

        self.dirty_load_positions.extend(load_positions);
        self.dirty_simulation_positions.extend(simulation_positions);
    }

    fn allocate_receipt(&mut self) -> ChunkTicketReceipt {
        self.latest_receipt = self.latest_receipt.next();
        self.latest_receipt
    }

    fn take_source_projections(
        &mut self,
        view_distance: u8,
        simulation_distance: u8,
    ) -> SourceProjectionBatch {
        let view_distance_changes = self
            .player_tickets
            .set_view_distance(&mut self.storage, view_distance);
        self.record_changes(view_distance_changes);
        let simulation_distance_changes = self
            .player_tickets
            .set_simulation_distance(&mut self.storage, simulation_distance);
        self.record_changes(simulation_distance_changes);

        let load_positions = Self::take_sorted_positions(&mut self.dirty_load_positions);
        let simulation_positions =
            Self::take_sorted_positions(&mut self.dirty_simulation_positions);
        let load_updates = load_positions
            .into_iter()
            .map(|pos| self.storage.load_source_update(pos))
            .collect();
        let simulation_updates = simulation_positions
            .into_iter()
            .map(|pos| self.storage.simulation_source_update(pos))
            .collect();

        SourceProjectionBatch {
            through_receipt: self.latest_receipt,
            load_updates,
            simulation_updates,
        }
    }

    fn take_sorted_positions(positions: &mut FxHashSet<ChunkPos>) -> Vec<ChunkPos> {
        let mut positions: Vec<_> = mem::take(positions).into_iter().collect();
        positions.sort_unstable_by_key(|pos| (pos.0.x, pos.0.y));
        positions
    }
}

#[derive(Debug)]
struct ChunkSchedulingTrackers {
    load: LoadTicketManager,
    simulation: SimulationTicketManager,
}

/// Owns authoritative ticket sources and their two propagation trackers.
///
/// Gameplay submissions only lock ingress. Update propagation locks trackers
/// first, snapshots both source projections under ingress, then releases
/// ingress before either tracker propagates.
pub(crate) struct ChunkSchedulingCoordinator {
    ticket_ingress: SyncMutex<TicketOperationIngress>,
    trackers: SyncMutex<ChunkSchedulingTrackers>,
    committed_receipt: AtomicU64,
}

impl ChunkSchedulingCoordinator {
    pub fn new(
        ticket_storage: ChunkTicketStorage,
        view_distance: u8,
        simulation_distance: u8,
    ) -> Self {
        let initial_load_sources = ticket_storage.initial_load_sources();
        let initial_simulation_sources = ticket_storage.initial_simulation_sources();
        let mut load = LoadTicketManager::new();
        load.apply_source_updates(initial_load_sources);
        let mut simulation = SimulationTicketManager::new();
        simulation.apply_source_updates(initial_simulation_sources);

        Self {
            ticket_ingress: SyncMutex::new(TicketOperationIngress::new(
                ticket_storage,
                view_distance,
                simulation_distance,
            )),
            trackers: SyncMutex::new(ChunkSchedulingTrackers { load, simulation }),
            committed_receipt: AtomicU64::new(ChunkTicketReceipt::INITIAL.0),
        }
    }

    pub(crate) fn queue_player_ticket_operation(
        &self,
        operation: PlayerTicketOperation,
    ) -> ChunkTicketReceipt {
        self.ticket_ingress.lock().push_player(operation)
    }

    pub(crate) fn queue_player_ticket_operations(
        &self,
        operations: impl IntoIterator<Item = PlayerTicketOperation>,
    ) -> Option<ChunkTicketReceipt> {
        self.ticket_ingress.lock().push_player_batch(operations)
    }

    pub(crate) fn acquire_chunk_request_leases(
        &self,
        positions: impl IntoIterator<Item = ChunkPos>,
        ticket_level: ChunkTicketLevel,
    ) -> Option<ChunkTicketReceipt> {
        self.ticket_ingress
            .lock()
            .acquire_chunk_request_leases(positions, ticket_level)
    }

    pub(crate) fn release_chunk_request_leases(
        &self,
        positions: impl IntoIterator<Item = ChunkPos>,
        ticket_level: ChunkTicketLevel,
    ) -> Option<ChunkTicketReceipt> {
        self.ticket_ingress
            .lock()
            .release_chunk_request_leases(positions, ticket_level)
    }

    pub fn run_all_updates(
        &self,
        view_distance: u8,
        simulation_distance: u8,
    ) -> ChunkSchedulingUpdateBatch {
        let mut trackers = self.trackers.lock();
        let projections = {
            let mut ingress = self.ticket_ingress.lock();
            ingress.take_source_projections(view_distance, simulation_distance)
        };

        trackers
            .simulation
            .apply_source_updates(projections.simulation_updates);
        trackers.simulation.run_all_updates();
        let simulation_changes = trackers.simulation.take_changes();

        trackers.load.apply_source_updates(projections.load_updates);
        trackers.load.run_all_updates();
        let load_changes = trackers.load.take_changes();

        ChunkSchedulingUpdateBatch {
            through_receipt: projections.through_receipt,
            load_changes,
            simulation_changes,
        }
    }

    pub(crate) fn add_or_refresh_portal_ticket(&self, pos: ChunkPos) {
        let mut ingress = self.ticket_ingress.lock();
        let changes = ingress.storage.add_or_refresh_portal_ticket(pos);
        ingress.record_changes(changes);
    }

    pub(crate) fn add_or_refresh_ender_pearl_ticket(&self, pos: ChunkPos) {
        let mut ingress = self.ticket_ingress.lock();
        let changes = ingress.storage.add_or_refresh_ender_pearl_ticket(pos);
        ingress.record_changes(changes);
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
        ingress.record_changes(changes);
    }

    #[must_use]
    pub(crate) fn persistent_chunk_tickets(&self) -> PersistentChunkTickets {
        self.ticket_ingress.lock().storage.to_persistent()
    }

    #[must_use]
    pub fn simulation_level(&self, pos: ChunkPos) -> Option<ChunkTicketLevel> {
        self.trackers.lock().simulation.get_level(pos)
    }

    pub fn recycle_update_batch(&self, batch: ChunkSchedulingUpdateBatch) {
        let mut trackers = self.trackers.lock();
        trackers.load.recycle_changes(batch.load_changes);
        trackers
            .simulation
            .recycle_changes(batch.simulation_changes);
    }

    pub fn publish_committed(&self, receipt: ChunkTicketReceipt) {
        let _ = self
            .committed_receipt
            .fetch_max(receipt.0, Ordering::Release);
    }

    #[must_use]
    pub fn is_receipt_committed(&self, receipt: ChunkTicketReceipt) -> bool {
        self.committed_receipt.load(Ordering::Acquire) >= receipt.0
    }
}

#[cfg(test)]
mod tests {
    use std::iter;

    use super::*;
    use crate::chunk::chunk_ticket_manager::ChunkTicketLevel;
    use uuid::Uuid;

    const TEST_VIEW_DISTANCE_CHUNKS: u8 = 4;
    const TEST_SIMULATION_DISTANCE_CHUNKS: u8 = 4;

    fn coordinator(simulation_distance: u8) -> ChunkSchedulingCoordinator {
        ChunkSchedulingCoordinator::new(
            ChunkTicketStorage::new(),
            TEST_VIEW_DISTANCE_CHUNKS,
            simulation_distance,
        )
    }

    fn has_load_change(
        changes: &[LoadLevelChange],
        pos: ChunkPos,
        new_level: Option<ChunkTicketLevel>,
    ) -> bool {
        changes
            .iter()
            .any(|change| change.pos == pos && change.new_level == new_level)
    }

    fn has_simulation_change(
        changes: &[SimulationLevelChange],
        pos: ChunkPos,
        new_level: Option<ChunkTicketLevel>,
    ) -> bool {
        changes
            .iter()
            .any(|change| change.pos == pos && change.new_level == new_level)
    }

    #[test]
    fn chunk_request_leases_share_one_ticket_and_keep_ordered_receipts() {
        let pos = ChunkPos::new(6, -9);
        let level = ChunkTicketLevel::FULL_CHUNK;
        let mut ingress = TicketOperationIngress::new(
            ChunkTicketStorage::new(),
            TEST_VIEW_DISTANCE_CHUNKS,
            TEST_SIMULATION_DISTANCE_CHUNKS,
        );

        let first_acquire = ingress
            .acquire_chunk_request_leases([pos], level)
            .expect("non-empty acquisition should produce a receipt");
        let second_acquire = ingress
            .acquire_chunk_request_leases([pos], level)
            .expect("non-empty acquisition should produce a receipt");
        let additions = ingress
            .take_source_projections(TEST_VIEW_DISTANCE_CHUNKS, TEST_SIMULATION_DISTANCE_CHUNKS);

        assert!(second_acquire > first_acquire);
        assert_eq!(additions.through_receipt, second_acquire);
        assert_eq!(
            additions.load_updates,
            [SourceLevelUpdate {
                pos,
                level: Some(level),
            }]
        );

        let first_release = ingress
            .release_chunk_request_leases([pos], level)
            .expect("non-empty release should produce a receipt");
        let retained = ingress
            .take_source_projections(TEST_VIEW_DISTANCE_CHUNKS, TEST_SIMULATION_DISTANCE_CHUNKS);
        assert!(first_release > second_acquire);
        assert_eq!(retained.through_receipt, first_release);
        assert_eq!(retained.load_updates, []);

        let final_release = ingress
            .release_chunk_request_leases([pos], level)
            .expect("non-empty release should produce a receipt");
        let removal = ingress
            .take_source_projections(TEST_VIEW_DISTANCE_CHUNKS, TEST_SIMULATION_DISTANCE_CHUNKS);
        assert!(final_release > first_release);
        assert_eq!(removal.through_receipt, final_release);
        assert_eq!(
            removal.load_updates,
            [SourceLevelUpdate { pos, level: None }]
        );
    }

    #[test]
    fn empty_batches_have_no_receipt_and_true_no_ops_are_barriers() {
        let coordinator = coordinator(TEST_SIMULATION_DISTANCE_CHUNKS);
        assert_eq!(
            coordinator.queue_player_ticket_operations(iter::empty()),
            None
        );

        let pos = ChunkPos::new(0, 0);
        let receipt = coordinator.queue_player_ticket_operation(PlayerTicketOperation::Remove {
            pos,
            player_id: Uuid::from_u128(1),
        });
        let batch =
            coordinator.run_all_updates(TEST_VIEW_DISTANCE_CHUNKS, TEST_SIMULATION_DISTANCE_CHUNKS);

        assert_eq!(receipt, ChunkTicketReceipt(1));
        assert_eq!(batch.through_receipt, receipt);
        assert_eq!(batch.load_changes, []);
        assert_eq!(batch.simulation_changes, []);
        assert!(!coordinator.is_receipt_committed(receipt));

        coordinator.publish_committed(batch.through_receipt);
        assert!(coordinator.is_receipt_committed(receipt));
    }

    #[test]
    fn unified_batch_and_receipt_commit_atomically() {
        let coordinator = coordinator(TEST_SIMULATION_DISTANCE_CHUNKS);
        let pos = ChunkPos::new(0, 0);
        let player_id = Uuid::from_u128(2);
        let receipt = coordinator
            .queue_player_ticket_operation(PlayerTicketOperation::Add { pos, player_id });

        let batch =
            coordinator.run_all_updates(TEST_VIEW_DISTANCE_CHUNKS, TEST_SIMULATION_DISTANCE_CHUNKS);

        assert_eq!(batch.through_receipt, receipt);
        assert!(has_load_change(
            &batch.load_changes,
            pos,
            Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
        ));
        assert!(has_simulation_change(
            &batch.simulation_changes,
            pos,
            Some(ChunkTicketLevel::for_entity_ticking_radius(
                TEST_SIMULATION_DISTANCE_CHUNKS
            ))
        ));
        assert_eq!(
            coordinator.simulation_level(pos),
            Some(ChunkTicketLevel::for_entity_ticking_radius(
                TEST_SIMULATION_DISTANCE_CHUNKS
            ))
        );
        assert!(!coordinator.is_receipt_committed(receipt));

        coordinator.publish_committed(batch.through_receipt);
        assert!(coordinator.is_receipt_committed(receipt));
        coordinator.recycle_update_batch(batch);

        let removal = coordinator
            .queue_player_ticket_operation(PlayerTicketOperation::Remove { pos, player_id });
        let batch =
            coordinator.run_all_updates(TEST_VIEW_DISTANCE_CHUNKS, TEST_SIMULATION_DISTANCE_CHUNKS);
        assert_eq!(removal, ChunkTicketReceipt(2));
        assert_eq!(batch.through_receipt, removal);
        assert!(has_load_change(&batch.load_changes, pos, None));
        assert!(has_simulation_change(&batch.simulation_changes, pos, None));
        assert!(!coordinator.is_receipt_committed(removal));

        coordinator.publish_committed(batch.through_receipt);
        assert!(coordinator.is_receipt_committed(removal));
    }

    #[test]
    fn simulation_distance_reprojects_players_without_allocating_a_receipt() {
        let initial_distance = 2;
        let updated_distance = 3;
        let coordinator = coordinator(initial_distance);
        let pos = ChunkPos::new(5, -3);
        let receipt = coordinator.queue_player_ticket_operation(PlayerTicketOperation::Add {
            pos,
            player_id: Uuid::from_u128(5),
        });

        let initial = coordinator.run_all_updates(TEST_VIEW_DISTANCE_CHUNKS, initial_distance);
        assert_eq!(initial.through_receipt, receipt);
        assert_eq!(
            coordinator.simulation_level(pos),
            Some(ChunkTicketLevel::for_entity_ticking_radius(
                initial_distance
            ))
        );

        let updated = coordinator.run_all_updates(TEST_VIEW_DISTANCE_CHUNKS, updated_distance);

        assert_eq!(updated.through_receipt, receipt);
        assert_eq!(updated.load_changes, []);
        assert!(has_simulation_change(
            &updated.simulation_changes,
            pos,
            Some(ChunkTicketLevel::for_entity_ticking_radius(
                updated_distance
            ))
        ));
        assert_eq!(
            coordinator.simulation_level(pos),
            Some(ChunkTicketLevel::for_entity_ticking_radius(
                updated_distance
            ))
        );
    }
}
