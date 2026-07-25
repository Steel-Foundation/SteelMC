use arc_swap::ArcSwap;
use rayon::ThreadPool;
use rustc_hash::{FxBuildHasher, FxHashMap, FxHashSet};
use std::{
    io, mem,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use steel_protocol::packet_traits::EncodedPacket;
use steel_protocol::packets::game::{
    BlockChange, CBlockUpdate, CLightUpdate, CSectionBlocksUpdate, CSetChunkCenter,
};
use steel_protocol::utils::ConnectionProtocol;
use steel_registry::dimension_type::DimensionTypeRef;
use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    fluid::FluidRef,
};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, PackedChunkPos, SectionPos, locks::SyncMutex};
use tokio::runtime::Runtime;
use tokio::sync::Notify;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::instrument;

use crate::behavior::{BLOCK_BEHAVIORS, FLUID_BEHAVIORS};
use crate::block_entity::{BlockEntityLifecycleExt as _, ClearedBlockEntities, SharedBlockEntity};
use crate::chunk::chunk_holder::{
    ChunkHolder, ChunkSaveDependency, PostProcessGenerationError, TickingReadiness,
};
pub(crate) use crate::chunk::chunk_scheduler::ChunkMapSchedulingTimings;
use crate::chunk::chunk_scheduler::{
    ChunkMapPreparationTimings, ChunkSchedulingBoundaryStep, ChunkSchedulingCoordinator,
    ChunkTicketOperation, ChunkTicketRevision, PreparedChunkSchedulingEpoch,
};
use crate::chunk::chunk_ticket_manager::{
    ChunkTicket, ChunkTicketLevel, ChunkTicketManager, ENDER_PEARL_TICKET_TIMEOUT_TICKS,
    LevelChange, PersistentChunkTickets, TimedChunkTickets, generation_status, is_block_ticking,
    is_entity_ticking, is_full,
};
use crate::chunk::full_chunk_readiness::{
    FullNeighborhoodCounts, FullNeighborhoodError, FullNeighborhoodIndex, FullPublication,
    FullPublicationQueue,
};
pub use crate::chunk::gameplay_chunk_lookup_cache::GameplayChunkLookupCacheStats;
use crate::chunk::gameplay_chunk_lookup_cache::{
    GameplayChunkLookupCacheScope, lookup_or_insert_with,
};
use crate::chunk::light::{
    LIGHT_CACHE_RADIUS, LightCacheLayout, LightCacheSetupRadius, LightLayer,
    LightSectionEmptinessChange, LightSectionRange, LightWorkWindowGate, LightWorkset,
    build_chunk_light_update_packet_for_sections,
    propagate_block_light_changes_with_empty_sections,
    propagate_sky_light_changes_with_empty_sections,
};
use crate::chunk::player_chunk_view::PlayerChunkView;
use crate::chunk::{
    chunk_access::{ChunkAccess, ChunkStatus},
    chunk_generation_task::ChunkGenerationTask,
    level_chunk::BlockRandomPositionGenerator,
    section::RandomTickSectionBits,
};
use crate::chunk_saver::ChunkStorage;
use crate::player::connection::NetworkConnection;
use crate::world::World;
use crate::world::tick_scheduler::{BlockTick, FluidTick, ScheduledTickRunBatch};
use crate::worldgen::{ChunkGeneratorType, WorldGenContext};
use crate::{entity::Entity, player::Player};

mod generation_readiness;
mod light_update_state;
mod light_updates;
mod persistence;
mod player_tracking;
mod scheduled_ticks;

#[cfg(test)]
use light_update_state::PendingLightUpdates;
use light_update_state::{InFlightLightUpdates, LightUpdateState, PendingChunkLightUpdates};

const GENERATION_THREAD_MULTIPLE: usize = 2;
// Vanilla applies this limit independently to block ticks and fluid ticks.
const MAX_SCHEDULED_TICKS_PER_TICK: usize = 65_536;

/// Lifetime, in ticks, of a thrown ender pearl's chunk ticket (vanilla
/// `TicketType.ENDER_PEARL` timeout). The pearl refreshes it every
/// `ENDER_PEARL_TICKET_TIMEOUT - 1` ticks while it flies.
pub const ENDER_PEARL_TICKET_TIMEOUT: u32 = ENDER_PEARL_TICKET_TIMEOUT_TICKS;

/// Timing information for the game tick portion of chunk map operations.
#[derive(Debug, Default)]
pub struct ChunkMapGameTickTimings {
    /// Time spent broadcasting block changes.
    pub broadcast_changes: Duration,
    /// Time spent collecting tickable chunks.
    pub collect_tickable: Duration,
    /// Time spent ticking chunks (random ticks, etc.).
    pub tick_chunks: Duration,
    /// Time spent ticking block entities.
    pub tick_block_entities: Duration,
    /// Number of block-ticking chunks.
    pub tickable_count: usize,
    /// Total number of loaded chunks.
    pub total_chunks: usize,
    /// Scoped holder-cache activity across the world game tick.
    pub lookup_cache: GameplayChunkLookupCacheStats,
}

#[derive(Clone)]
struct TickableChunk {
    pos: ChunkPos,
    holder: Arc<ChunkHolder>,
    randomly_ticking_sections: Arc<RandomTickSectionBits>,
}

/// Immutable views of the chunk sets consumed during a game tick.
///
/// Entries retain the optimized SCC traversal order captured at the last
/// membership-changing lifecycle boundary. This is also Steel's documented
/// final order for the implementation-specific cross-chunk ties that Vanilla
/// derives from its fastutil map state.
#[derive(Default)]
struct TickingChunkSnapshot {
    block: Box<[TickableChunk]>,
    random_chunk_indices: Box<[usize]>,
    entity_indices: Box<[usize]>,
}

struct FinalizedBlockEntityUnload {
    holder: Arc<ChunkHolder>,
    lifecycle_dispatchers: Vec<SharedBlockEntity>,
    positions: Vec<BlockPos>,
}

struct BlockTickBatchGuard<'a> {
    world: &'a World,
    batch: Arc<ScheduledTickRunBatch<BlockRef>>,
}

impl<'a> BlockTickBatchGuard<'a> {
    fn new(world: &'a World, ticks: Vec<BlockTick>) -> Self {
        Self {
            world,
            batch: world.begin_scheduled_block_tick_batch(ticks),
        }
    }

    fn ticks(&self) -> &[BlockTick] {
        self.batch.ticks()
    }

    fn start(&self, index: usize) {
        self.batch.start(index);
    }
}

impl Drop for BlockTickBatchGuard<'_> {
    fn drop(&mut self) {
        self.world.end_scheduled_block_tick_batch(&self.batch);
    }
}

struct FluidTickBatchGuard<'a> {
    world: &'a World,
    batch: Arc<ScheduledTickRunBatch<FluidRef>>,
}

impl<'a> FluidTickBatchGuard<'a> {
    fn new(world: &'a World, ticks: Vec<FluidTick>) -> Self {
        Self {
            world,
            batch: world.begin_scheduled_fluid_tick_batch(ticks),
        }
    }

    fn ticks(&self) -> &[FluidTick] {
        self.batch.ticks()
    }

    fn start(&self, index: usize) {
        self.batch.start(index);
    }
}

impl Drop for FluidTickBatchGuard<'_> {
    fn drop(&mut self) {
        self.world.end_scheduled_fluid_tick_batch(&self.batch);
    }
}

struct TickingReadinessCandidate {
    pos: ChunkPos,
    holder: Arc<ChunkHolder>,
    desired: TickingReadiness,
    target: TickingReadiness,
}

#[derive(Default)]
struct ReadinessReconcileResult {
    snapshot_changed: bool,
    post_process_generation: Duration,
    post_process_chunk_count: usize,
    post_process_position_count: usize,
    candidate_count: usize,
}

/// A map of chunks managing their state, loading, and generation.
pub struct ChunkMap {
    /// Map of active chunks.
    pub(crate) chunks: scc::HashMap<ChunkPos, Arc<ChunkHolder>, FxBuildHasher>,
    /// Map of chunks currently being unloaded.
    pub(crate) unloading_chunks: scc::HashMap<ChunkPos, Arc<ChunkHolder>, FxBuildHasher>,
    /// Queue of pending generation tasks.
    pub pending_generation_tasks: SyncMutex<Vec<Arc<ChunkGenerationTask>>>,
    /// Tracker for background scheduling, generation, save, and unload tasks.
    pub task_tracker: TaskTracker,
    /// Ordered ticket ingress and background scheduling epoch handoff.
    scheduling: ChunkSchedulingCoordinator,
    /// Full status completions awaiting lifecycle-boundary reconciliation.
    full_publications: Arc<FullPublicationQueue>,
    /// Incremental radius-1/radius-2 Full-neighborhood state.
    full_neighborhood: SyncMutex<FullNeighborhoodIndex>,
    /// Readiness-driven chunk views published at lifecycle boundaries.
    ticking_chunks: ArcSwap<TickingChunkSnapshot>,
    /// Final-unload callbacks waiting for the serialized lifecycle boundary.
    finalized_block_entity_unloads: SyncMutex<Vec<FinalizedBlockEntityUnload>>,
    /// Timed gameplay ticket owners that expire through the game tick.
    timed_chunk_tickets: SyncMutex<TimedChunkTickets>,
    /// The world generation context.
    pub world_gen_context: Arc<WorldGenContext>,
    /// The thread pool to use for chunk generation (throughput-oriented).
    pub generation_pool: Arc<ThreadPool>,
    /// The thread pool to use for chunk ticking (latency-oriented).
    //pub tick_pool: Arc<ThreadPool>,
    /// The runtime to use for chunk tasks.
    pub chunk_runtime: Arc<Runtime>,
    /// Storage backend for chunk saving and loading.
    pub storage: Arc<ChunkStorage>,
    /// Chunk holders with pending block changes to broadcast.
    pub chunks_to_broadcast: SyncMutex<Vec<Arc<ChunkHolder>>>,
    /// Coalesced light changes and drained-but-not-yet-applied light work.
    light_updates: SyncMutex<LightUpdateState>,
    /// Notifies save barriers when in-flight light propagation state changes.
    light_updates_progress_notify: Notify,
    /// Radius-2 work-window gate for light-engine worksets.
    light_work_window_gate: Arc<LightWorkWindowGate>,
    /// Number of top-level generation tasks currently running.
    running_generation_tasks: AtomicUsize,
    /// Wakes the generation refill loop when pending/running task state changes.
    generation_refill_notify: Notify,
    /// Cancels the generation refill loop without cancelling active generation tasks.
    generation_refill_cancel_token: CancellationToken,
    /// Fast shutdown flag for the generation refill loop.
    generation_refill_stopped: AtomicBool,
    /// Whether the notify-driven refill loop has been started for this map.
    generation_refill_started: AtomicBool,
    /// Parent cancellation token for all generation tasks.
    /// Child tokens are created per-task; cancelling this cancels everything.
    pub cancel_token: CancellationToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct GenerationTaskPriority {
    simulation_bucket: u8,
    simulation_level: ChunkTicketLevel,
    load_level: ChunkTicketLevel,
}

impl GenerationTaskPriority {
    const fn for_levels(
        load_level: Option<ChunkTicketLevel>,
        simulation_level: Option<ChunkTicketLevel>,
    ) -> Self {
        let simulation_bucket = if simulation_level.is_some() { 0 } else { 1 };
        Self {
            simulation_bucket,
            simulation_level: match simulation_level {
                Some(level) => level,
                None => ChunkTicketLevel::MAX,
            },
            load_level: match load_level {
                Some(level) => level,
                None => ChunkTicketLevel::MAX,
            },
        }
    }
}

struct RunningGenerationTaskPermit {
    chunk_map: Arc<ChunkMap>,
}

impl Drop for RunningGenerationTaskPermit {
    fn drop(&mut self) {
        self.chunk_map
            .running_generation_tasks
            .fetch_sub(1, Ordering::AcqRel);
        self.chunk_map.notify_generation_refill();
    }
}

impl ChunkMap {
    /// Creates a new chunk map with a custom storage backend.
    ///
    /// This allows using different storage implementations (disk, RAM, etc.).
    #[must_use]
    pub fn new_with_storage(
        chunk_runtime: Arc<Runtime>,
        world: Weak<World>,
        dimension_type: DimensionTypeRef,
        sea_level: i32,
        storage: Arc<ChunkStorage>,
        generator: Arc<ChunkGeneratorType>,
        generation_pool: Arc<ThreadPool>,
    ) -> Self {
        Self::new_with_storage_and_timed_tickets(
            chunk_runtime,
            world,
            dimension_type,
            sea_level,
            storage,
            generator,
            generation_pool,
            TimedChunkTickets::default(),
        )
    }

    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "extends ChunkMap::new_with_storage with restored runtime ticket state"
    )]
    pub(crate) fn new_with_storage_and_timed_tickets(
        chunk_runtime: Arc<Runtime>,
        world: Weak<World>,
        dimension_type: DimensionTypeRef,
        sea_level: i32,
        storage: Arc<ChunkStorage>,
        generator: Arc<ChunkGeneratorType>,
        generation_pool: Arc<ThreadPool>,
        timed_chunk_tickets: TimedChunkTickets,
    ) -> Self {
        let mut chunk_tickets = ChunkTicketManager::new();
        timed_chunk_tickets.activate_all(&mut chunk_tickets);
        let full_publications = Arc::new(FullPublicationQueue::default());

        Self {
            chunks: scc::HashMap::default(),
            unloading_chunks: scc::HashMap::default(),
            pending_generation_tasks: SyncMutex::new(Vec::new()),
            task_tracker: TaskTracker::new(),
            scheduling: ChunkSchedulingCoordinator::new(chunk_tickets),
            full_publications,
            full_neighborhood: SyncMutex::new(FullNeighborhoodIndex::default()),
            ticking_chunks: ArcSwap::from_pointee(TickingChunkSnapshot::default()),
            finalized_block_entity_unloads: SyncMutex::new(Vec::new()),
            timed_chunk_tickets: SyncMutex::new(timed_chunk_tickets),
            world_gen_context: Arc::new(WorldGenContext::new(
                generator,
                world,
                dimension_type.min_y,
                dimension_type.height,
                sea_level,
            )),
            generation_pool,
            chunk_runtime,
            storage,
            chunks_to_broadcast: SyncMutex::new(Vec::new()),
            light_updates: SyncMutex::new(LightUpdateState::default()),
            light_updates_progress_notify: Notify::new(),
            light_work_window_gate: Arc::new(LightWorkWindowGate::new()),
            running_generation_tasks: AtomicUsize::new(0),
            generation_refill_notify: Notify::new(),
            generation_refill_cancel_token: CancellationToken::new(),
            generation_refill_stopped: AtomicBool::new(false),
            generation_refill_started: AtomicBool::new(false),
            cancel_token: CancellationToken::new(),
        }
    }

    pub(crate) fn light_work_window_gate(&self) -> Arc<LightWorkWindowGate> {
        Arc::clone(&self.light_work_window_gate)
    }

    /// Starts the notify-driven generation refill loop for this chunk map.
    pub fn start_generation_refill_loop(self: &Arc<Self>) {
        if self.generation_refill_started.swap(true, Ordering::AcqRel) {
            return;
        }

        let chunk_map = Arc::clone(self);
        self.task_tracker.spawn_on(
            async move {
                loop {
                    tokio::select! {
                        () = chunk_map.generation_refill_cancel_token.cancelled() => break,
                        () = chunk_map.generation_refill_notify.notified() => {
                            chunk_map.run_generation_tasks_b();
                        }
                    }
                }
            },
            self.chunk_runtime.handle(),
        );
    }

    /// Stops the generation refill loop. Active generation tasks are left alone.
    pub fn stop_generation_refill_loop(&self) {
        self.generation_refill_stopped
            .store(true, Ordering::Release);
        self.generation_refill_cancel_token.cancel();
        self.generation_refill_notify.notify_waiters();
    }

    pub(crate) fn notify_generation_refill(&self) {
        self.generation_refill_notify.notify_one();
    }

    fn run_or_notify_generation_refill(&self) {
        if self.generation_refill_started.load(Ordering::Acquire) {
            self.notify_generation_refill();
        } else {
            self.run_generation_tasks_b();
        }
    }

    /// Executes a function with access to a fully loaded chunk.
    /// Returns `None` if the chunk is not loaded or not at Full status.
    pub fn with_full_chunk<F, R>(&self, pos: ChunkPos, f: F) -> Option<R>
    where
        F: FnOnce(&ChunkAccess) -> R,
    {
        self.with_chunk_at_status(pos, ChunkStatus::Full, f)
    }

    /// Inserts a non-simulated holder into an empty gameplay view for worldgen benchmarks.
    ///
    /// Runtime lifecycle code must use ticket-driven insertion. Benchmark holders
    /// cannot enter a ticking snapshot, so bulk fixture construction needs no rebuild.
    #[doc(hidden)]
    #[cfg(feature = "benchmark-support")]
    pub fn insert_benchmark_chunk_holder(&self, pos: ChunkPos, holder: Arc<ChunkHolder>) {
        assert!(holder.simulation_level().is_none());
        assert!(self.ticking_chunks.load().block.is_empty());
        let _ = self.chunks.insert_sync(pos, holder);
    }

    #[inline]
    fn lookup_active_holder(&self, pos: ChunkPos) -> Option<Arc<ChunkHolder>> {
        lookup_or_insert_with(self, pos, || {
            self.chunks.read_sync(&pos, |_, holder| Arc::clone(holder))
        })
    }

    /// Returns whether an active full chunk is currently block ticking.
    #[must_use]
    pub(crate) fn is_block_ticking_full_chunk_loaded(&self, pos: ChunkPos) -> bool {
        self.lookup_active_holder(pos).is_some_and(|holder| {
            is_block_ticking(holder.load_level())
                && holder.ticking_readiness_snapshot().is_block_ticking()
        })
    }

    /// Returns whether the chunk is in block simulation range with confirmed r1 readiness.
    #[must_use]
    pub(crate) fn is_block_ticking_full_chunk_simulated(&self, pos: ChunkPos) -> bool {
        self.lookup_active_holder(pos).is_some_and(|holder| {
            is_block_ticking(holder.simulation_level())
                && holder.ticking_readiness_snapshot().is_block_ticking()
        })
    }

    /// Executes a function with access to a chunk at the requested generation status or later.
    /// Returns `None` if the chunk is not loaded or has not reached the requested status.
    pub(crate) fn with_chunk_at_status<F, R>(
        &self,
        pos: ChunkPos,
        status: ChunkStatus,
        f: F,
    ) -> Option<R>
    where
        F: FnOnce(&ChunkAccess) -> R,
    {
        let chunk_holder = self.lookup_active_holder(pos)?;
        // Holders retain completed higher-status data for saving and quick revival. Gameplay
        // lookups must still honor the currently permitted generation status.
        if chunk_holder.is_status_disallowed(status) {
            return None;
        }
        let guard = chunk_holder.try_chunk(status)?;
        Some(f(&guard))
    }

    pub(crate) fn add_chunk_ticket(
        &self,
        pos: ChunkPos,
        ticket: ChunkTicket,
    ) -> ChunkTicketRevision {
        self.scheduling
            .queue_ticket_operation(ChunkTicketOperation::Add { pos, ticket })
    }

    pub(crate) fn add_chunk_tickets(
        &self,
        positions: &[ChunkPos],
        ticket: ChunkTicket,
    ) -> Option<ChunkTicketRevision> {
        self.scheduling.queue_ticket_operations(
            positions
                .iter()
                .copied()
                .map(|pos| ChunkTicketOperation::Add { pos, ticket }),
        )
    }

    pub(crate) fn remove_chunk_ticket(
        &self,
        pos: ChunkPos,
        ticket: ChunkTicket,
    ) -> ChunkTicketRevision {
        self.scheduling
            .queue_ticket_operation(ChunkTicketOperation::Remove { pos, ticket })
    }

    pub(crate) fn remove_chunk_tickets(
        &self,
        positions: &[ChunkPos],
        ticket: ChunkTicket,
    ) -> Option<ChunkTicketRevision> {
        self.scheduling.queue_ticket_operations(
            positions
                .iter()
                .copied()
                .map(|pos| ChunkTicketOperation::Remove { pos, ticket }),
        )
    }

    fn replace_chunk_ticket(
        &self,
        old_pos: ChunkPos,
        old_ticket: ChunkTicket,
        new_pos: ChunkPos,
        new_ticket: ChunkTicket,
    ) {
        let operations = [
            ChunkTicketOperation::Remove {
                pos: old_pos,
                ticket: old_ticket,
            },
            ChunkTicketOperation::Add {
                pos: new_pos,
                ticket: new_ticket,
            },
        ];
        let _ = self.scheduling.queue_ticket_operations(operations);
    }

    pub(crate) fn is_ticket_revision_committed(&self, revision: ChunkTicketRevision) -> bool {
        self.scheduling.is_revision_committed(revision)
    }

    /// Drives startup scheduling until a full square is ready, runs `f`, then
    /// removes the temporary ticket.
    pub(crate) async fn with_full_chunks_in_radius<F, R>(
        self: &Arc<Self>,
        center: ChunkPos,
        radius: u8,
        f: F,
    ) -> Option<R>
    where
        F: FnOnce() -> R,
    {
        let ticket = ChunkTicket::full_chunks(radius);

        let ticket_revision = self.add_chunk_ticket(center, ticket);
        let radius = i32::from(radius);

        loop {
            self.advance_scheduling();
            if self.is_ticket_revision_committed(ticket_revision)
                && self.full_square_is_ready(center, radius)
            {
                break;
            }

            if self.cancel_token.is_cancelled() {
                self.remove_chunk_ticket(center, ticket);
                self.advance_scheduling();
                return None;
            }

            sleep(Duration::from_millis(10)).await;
        }

        let result = f();
        self.remove_chunk_ticket(center, ticket);
        self.advance_scheduling();

        Some(result)
    }

    /// Adds or refreshes vanilla's post-portal chunk ticket.
    pub(crate) fn place_portal_ticket(&self, ticket_position: BlockPos) {
        let center = ChunkPos::from_block_pos(ticket_position);
        let mut timed_tickets = self.timed_chunk_tickets.lock();
        let ticket = timed_tickets.add_portal_ticket(center);
        if let Some(ticket) = ticket {
            self.add_chunk_ticket(center, ticket);
        }
    }

    /// Advances gameplay-owned timed chunk tickets by one server tick.
    pub(crate) fn tick_timed_tickets(&self) {
        let mut timed_tickets = self.timed_chunk_tickets.lock();
        let expired = timed_tickets.tick(|pos| self.can_timed_ticket_expire(pos));
        let _ = self.scheduling.queue_ticket_operations(
            expired
                .into_iter()
                .map(|(pos, ticket)| ChunkTicketOperation::Remove { pos, ticket }),
        );
    }

    pub(crate) fn persistent_chunk_tickets(&self) -> PersistentChunkTickets {
        self.timed_chunk_tickets.lock().to_persistent()
    }

    fn can_timed_ticket_expire(&self, pos: ChunkPos) -> bool {
        self.chunks
            .read_sync(&pos, |_, holder| holder.is_ready_for_saving())
            .unwrap_or(true)
    }

    fn full_square_is_ready(&self, center: ChunkPos, radius: i32) -> bool {
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let pos = ChunkPos::new(center.0.x + dx, center.0.y + dz);
                let Some(holder) = self.chunks.read_sync(&pos, |_, holder| holder.clone()) else {
                    return false;
                };
                if holder.try_chunk(ChunkStatus::Full).is_none() {
                    return false;
                }
            }
        }
        true
    }

    /// Broadcasts pending block changes and completed light changes to nearby players.
    #[expect(
        clippy::too_many_lines,
        reason = "block and light packet construction share the same holder drain"
    )]
    pub fn broadcast_changed_chunks(&self) {
        self.propagate_queued_light_changes();

        let holders = {
            let mut guard = self.chunks_to_broadcast.lock();
            if guard.is_empty() {
                return;
            }
            mem::take(&mut *guard)
        };

        let mut world = None;

        for holder in holders {
            let chunk_pos = holder.get_pos();
            // Vanilla publishes block changes independently of unfinished light propagation.
            let world = world.get_or_insert_with(|| self.world_gen_context.world());
            let has_skylight = world.dimension_type.has_skylight;
            let min_y = holder.min_y();
            holder.clear_broadcast_queued();

            let light_changes = holder.take_changed_light_sections();
            // Take all pending changes from this chunk holder
            let changes_by_section = holder.take_changed_blocks();
            let has_publishable_light_changes =
                !light_changes.block.is_empty() || (has_skylight && !light_changes.sky.is_empty());

            if !has_publishable_light_changes && changes_by_section.is_empty() {
                continue;
            }

            if has_publishable_light_changes
                && let Some(chunk) = holder.try_chunk(ChunkStatus::Full)
            {
                let tracking_players = world.get_light_packet_tracking_players(chunk_pos);
                if !tracking_players.is_empty() {
                    let light_data = {
                        let light = chunk.light();
                        let sky_sections = if has_skylight {
                            light_changes.sky.as_slice()
                        } else {
                            &[]
                        };
                        build_chunk_light_update_packet_for_sections(
                            chunk_pos,
                            &light,
                            has_skylight,
                            sky_sections,
                            &light_changes.block,
                        )
                    };
                    let light_packet = CLightUpdate {
                        x: chunk_pos.0.x,
                        z: chunk_pos.0.y,
                        light_data,
                    };

                    let Ok(encoded) = EncodedPacket::from_bare(
                        light_packet,
                        world.compression,
                        ConnectionProtocol::Play,
                    ) else {
                        log::warn!("Failed to encode light update packet");
                        continue;
                    };

                    for entity_id in &tracking_players {
                        if let Some(player) = world.players.get_by_entity_id(*entity_id) {
                            player.connection.send_encoded(encoded.clone());
                        }
                    }
                }
            }

            if changes_by_section.is_empty() {
                continue;
            }

            // Get players whose client already has the base chunk packet.
            let tracking_players = world.get_packet_tracking_players(chunk_pos);
            if tracking_players.is_empty() {
                continue;
            }

            // For each section with changes, send appropriate packet
            for (section_index, changed_positions) in changes_by_section {
                let section_y = min_y / 16 + section_index as i32;
                let section_pos = SectionPos::new(chunk_pos.0.x, section_y, chunk_pos.0.y);

                if changed_positions.len() == 1 {
                    // Single block change - use CBlockUpdate
                    let Some(&packed) = changed_positions.iter().next() else {
                        continue;
                    };
                    let block_pos = section_pos.relative_to_block_pos(packed);
                    let block_state = world.get_block_state(block_pos);

                    tracing::trace!(
                        ?block_pos,
                        ?block_state,
                        player_count = tracking_players.len(),
                        "Broadcasting single block update"
                    );

                    let update_packet = CBlockUpdate {
                        pos: block_pos,
                        block_state,
                    };

                    let Ok(encoded) = EncodedPacket::from_bare(
                        update_packet,
                        world.compression,
                        ConnectionProtocol::Play,
                    ) else {
                        log::warn!("Failed to encode block update packet");
                        continue;
                    };

                    for entity_id in &tracking_players {
                        if let Some(player) = world.players.get_by_entity_id(*entity_id) {
                            player.connection.send_encoded(encoded.clone());
                        }
                    }
                    world.broadcast_block_entity_if_needed(block_pos);
                } else {
                    // Multiple block changes - use CSectionBlocksUpdate
                    let changes: Vec<BlockChange> = changed_positions
                        .iter()
                        .map(|&packed| {
                            let block_pos = section_pos.relative_to_block_pos(packed);
                            let block_state = world.get_block_state(block_pos);
                            BlockChange {
                                pos: packed,
                                block_state,
                            }
                        })
                        .collect();

                    tracing::trace!(
                        change_count = changes.len(),
                        ?section_pos,
                        player_count = tracking_players.len(),
                        "Broadcasting section block updates"
                    );

                    let packet = CSectionBlocksUpdate {
                        section_pos,
                        changes,
                    };

                    let Ok(encoded) = EncodedPacket::from_bare(
                        packet,
                        world.compression,
                        ConnectionProtocol::Play,
                    ) else {
                        log::warn!("Failed to encode section block update packet");
                        continue;
                    };

                    for entity_id in &tracking_players {
                        if let Some(player) = world.players.get_by_entity_id(*entity_id) {
                            player.connection.send_encoded(encoded.clone());
                        }
                    }
                    for &packed in &changed_positions {
                        let block_pos = section_pos.relative_to_block_pos(packed);
                        world.broadcast_block_entity_if_needed(block_pos);
                    }
                }
            }
        }
    }

    /// Processes chunk updates, ticks chunks, and executes ready scheduled ticks.
    ///
    /// # Arguments
    /// * `world` - The world reference (needed for executing scheduled tick callbacks)
    /// Game tick: broadcasts block changes, ticks chunks (random + scheduled ticks).
    ///
    /// Runs on the main game tick loop. Does NOT handle chunk generation or unloading.
    #[instrument(level = "trace", skip(self, world), name = "chunk_map_game_tick")]
    pub fn tick_game(
        self: &Arc<Self>,
        world: &Arc<World>,
        tick_count: u64,
        random_tick_speed: u32,
        runs_normally: bool,
    ) -> ChunkMapGameTickTimings {
        let mut timings = ChunkMapGameTickTimings::default();

        if tick_count.is_multiple_of(100) {
            tracing::debug!(
                chunks = self.chunks.len(),
                unloading = self.unloading_chunks.len(),
                "Chunk map status"
            );
        }

        if !runs_normally {
            let _span = tracing::trace_span!("broadcast_changes").entered();
            let start = Instant::now();
            self.broadcast_changed_chunks();
            timings.broadcast_changes = start.elapsed();
            return timings;
        }

        {
            let _span = tracing::trace_span!("collect_tickable").entered();
            let start = Instant::now();
            let tickable_chunks = self.ticking_chunks.load();
            timings.collect_tickable = start.elapsed();
            timings.total_chunks = self.chunks.len();
            timings.tickable_count = tickable_chunks.block.len();

            if !tickable_chunks.block.is_empty() {
                let _span = tracing::trace_span!(
                    "tick_chunks",
                    block_ticking_count = tickable_chunks.block.len(),
                    total_chunks = timings.total_chunks
                )
                .entered();
                let start = Instant::now();
                // Block and fluid collection share the same post-`tick_time`
                // timestamp even though block callbacks run between the phases.
                let current_tick = world.game_time();
                let ready_block_ticks =
                    Self::collect_scheduled_block_ticks(world, &tickable_chunks, current_tick);
                Self::execute_scheduled_block_ticks(world, ready_block_ticks);

                let ready_fluid_ticks =
                    Self::collect_scheduled_fluid_ticks(world, &tickable_chunks, current_tick);
                Self::execute_scheduled_fluid_ticks(world, ready_fluid_ticks);

                if random_tick_speed > 0 {
                    // Intentional Steel difference: this uses Vanilla's coordinate LCG,
                    // but seeds it per tick from runtime RNG instead of sharing Level RNG.
                    let mut random_positions = BlockRandomPositionGenerator::from_runtime_rng();
                    for &index in &tickable_chunks.random_chunk_indices {
                        // Vanilla random chunk ticks use the entity-ticking range but only
                        // require the same confirmed block-ticking chunk used by scheduled ticks.
                        let tickable_chunk = &tickable_chunks.block[index];
                        if tickable_chunk.randomly_ticking_sections.is_empty() {
                            continue;
                        }
                        if let Some(chunk_guard) =
                            tickable_chunk.holder.try_chunk(ChunkStatus::Full)
                        {
                            chunk_guard.tick_random_blocks(
                                world,
                                random_tick_speed,
                                &mut random_positions,
                            );
                        }
                    }
                }
                timings.tick_chunks = start.elapsed();
            }
        }

        {
            let _span = tracing::trace_span!("broadcast_changes").entered();
            let start = Instant::now();
            self.broadcast_changed_chunks();
            timings.broadcast_changes = start.elapsed();
        }

        timings
    }

    /// Ticks block entities in tickable full chunks.
    /// Commits a ready scheduling epoch and forks the next background epoch.
    ///
    /// This must run at a gameplay lifecycle boundary or during startup before
    /// gameplay begins. It never waits for a running epoch; the previously
    /// committed chunk state remains authoritative until that epoch is ready at
    /// a later boundary.
    #[instrument(level = "trace", skip(self), name = "advance_chunk_scheduling")]
    pub(crate) fn advance_scheduling(self: &Arc<Self>) -> ChunkMapSchedulingTimings {
        match self.scheduling.take_boundary_step() {
            ChunkSchedulingBoundaryStep::Running => ChunkMapSchedulingTimings::default(),
            ChunkSchedulingBoundaryStep::Start {
                ticket_manager,
                applied_revision,
            } => {
                self.spawn_scheduling_epoch(ticket_manager, applied_revision, Vec::new());
                ChunkMapSchedulingTimings::default()
            }
            ChunkSchedulingBoundaryStep::Commit(epoch) => self.commit_scheduling_epoch(epoch),
        }
    }

    fn commit_scheduling_epoch(
        self: &Arc<Self>,
        epoch: PreparedChunkSchedulingEpoch,
    ) -> ChunkMapSchedulingTimings {
        let PreparedChunkSchedulingEpoch {
            mut ticket_manager,
            applied_revision,
            mut changes,
            timings,
        } = epoch;
        let mut timings = timings.into_scheduling_timings();

        {
            let _span = tracing::trace_span!("block_entity_unloads").entered();
            let start = Instant::now();
            // Finalized old holders leave the block-entity world before a new holder at
            // the same position can be committed and activated below.
            self.finish_block_entity_unloads();
            timings.block_entity_unloads = start.elapsed();
        }

        let (changed_positions, mut rebuild_ticking_snapshot, rebuild_readiness) = {
            let _span = tracing::trace_span!("readiness_demotions").entered();
            let start = Instant::now();
            let changed_positions = changes.iter().map(|change| change.pos).collect::<Vec<_>>();
            let mut rebuild_ticking_snapshot = self.simulation_changes_ticking_snapshot(&changes);
            let rebuild_readiness = match self.prepare_ticking_readiness_demotions(&changes) {
                Ok(changed) => {
                    rebuild_ticking_snapshot |= changed;
                    false
                }
                Err(error) => {
                    tracing::error!(
                        ?error,
                        "Full-neighborhood index invariant failed before lifecycle commit; rebuilding after the commit"
                    );
                    self.clear_all_ticking_readiness();
                    *self.full_neighborhood.lock() = FullNeighborhoodIndex::default();
                    true
                }
            };
            timings.readiness_demotions = start.elapsed();
            (
                changed_positions,
                rebuild_ticking_snapshot,
                rebuild_readiness,
            )
        };

        let holders_to_schedule = {
            let _span = tracing::trace_span!("lifecycle_commit").entered();
            let start = Instant::now();
            let holders = changes
                .drain(..)
                .filter_map(|change| {
                    self.update_chunk_level(
                        change.pos,
                        change.new_level,
                        change.new_simulation_level,
                    )
                    .zip(change.new_level)
                })
                .collect();
            timings.lifecycle_commit = start.elapsed();
            holders
        };

        let lookup_cache_scope = GameplayChunkLookupCacheScope::enter(self);
        let readiness_result = {
            let _span = tracing::trace_span!("readiness_reconcile").entered();
            let start = Instant::now();
            let result = if rebuild_readiness {
                rebuild_ticking_snapshot = true;
                match self.rebuild_ticking_readiness() {
                    Ok(result) => result,
                    Err(error) => self.recover_ticking_readiness_index(error),
                }
            } else {
                match self.reconcile_ticking_readiness_measured(&changed_positions) {
                    Ok(result) => {
                        rebuild_ticking_snapshot |= result.snapshot_changed;
                        result
                    }
                    Err(error) => {
                        rebuild_ticking_snapshot = true;
                        self.recover_ticking_readiness_index(error)
                    }
                }
            };
            timings.readiness_reconcile = start.elapsed();
            result
        };
        timings.lookup_cache = lookup_cache_scope.finish();
        timings.post_process_generation = readiness_result.post_process_generation;
        timings.post_process_chunk_count = readiness_result.post_process_chunk_count;
        timings.post_process_position_count = readiness_result.post_process_position_count;
        timings.readiness_candidate_count = readiness_result.candidate_count;

        if rebuild_ticking_snapshot {
            let _span = tracing::trace_span!("ticking_snapshot_rebuild").entered();
            let start = Instant::now();
            timings.rebuilt_ticking_chunk_count = self.rebuild_ticking_chunk_snapshot();
            timings.ticking_snapshot_rebuild = start.elapsed();
        }

        ticket_manager.recycle_changes(changes);
        self.scheduling.publish_committed_revision(applied_revision);
        self.spawn_scheduling_epoch(ticket_manager, applied_revision, holders_to_schedule);
        timings
    }

    fn spawn_scheduling_epoch(
        self: &Arc<Self>,
        ticket_manager: ChunkTicketManager,
        applied_revision: ChunkTicketRevision,
        holders_to_schedule: Vec<(Arc<ChunkHolder>, ChunkTicketLevel)>,
    ) {
        let chunk_map = Arc::clone(self);
        // The task tracker owns shutdown accounting; the join handle is not needed.
        drop(self.task_tracker.spawn_blocking_on(
            move || {
                let epoch = chunk_map.prepare_scheduling_epoch(
                    ticket_manager,
                    applied_revision,
                    holders_to_schedule,
                );
                chunk_map.scheduling.finish_epoch(epoch);
            },
            self.chunk_runtime.handle(),
        ));
    }

    #[instrument(level = "trace", skip(self, ticket_manager, holders_to_schedule))]
    fn prepare_scheduling_epoch(
        self: &Arc<Self>,
        mut ticket_manager: ChunkTicketManager,
        applied_revision: ChunkTicketRevision,
        holders_to_schedule: Vec<(Arc<ChunkHolder>, ChunkTicketLevel)>,
    ) -> PreparedChunkSchedulingEpoch {
        let mut timings = ChunkMapPreparationTimings::default();

        let applied_revision = {
            let _span = tracing::trace_span!("ticket_updates").entered();
            let start = Instant::now();
            let revision = self
                .scheduling
                .apply_pending_ticket_operations(&mut ticket_manager, applied_revision);
            ticket_manager.run_all_updates();
            timings.ticket_updates = start.elapsed();
            revision
        };
        let changes = ticket_manager.take_changes();

        {
            let _span = tracing::trace_span!("schedule_generation").entered();
            let start = Instant::now();
            timings.scheduled_count = holders_to_schedule
                .iter()
                .filter(|(holder, level)| {
                    let Some(status) = generation_status(Some(*level)) else {
                        return false;
                    };
                    holder.schedule_chunk_generation_task_b(status, self)
                })
                .count();
            timings.schedule_generation = start.elapsed();
        }

        {
            let _span = tracing::trace_span!("run_generation").entered();
            let start = Instant::now();
            self.run_or_notify_generation_refill();
            timings.run_generation = start.elapsed();
        }

        {
            let _span = tracing::trace_span!("process_unloads").entered();
            let start = Instant::now();
            let staged_revivals = changes
                .iter()
                .filter(|change| {
                    change.new_level.is_some() && self.unloading_chunks.contains_sync(&change.pos)
                })
                .map(|change| change.pos)
                .collect::<FxHashSet<_>>();
            self.process_unloads(&staged_revivals);
            timings.process_unloads = start.elapsed();
        }

        PreparedChunkSchedulingEpoch {
            ticket_manager,
            applied_revision,
            changes,
            timings,
        }
    }

    /// Returns full chunks whose simulation level currently allows entity ticks.
    pub fn tickable_full_chunk_positions(&self) -> Vec<ChunkPos> {
        let snapshot = self.ticking_chunks.load();
        snapshot
            .entity_indices
            .iter()
            .map(|&index| snapshot.block[index].pos)
            .collect()
    }

    /// Returns whether the chunk is full and currently allows entity ticks.
    pub(crate) fn is_entity_ticking_full_chunk_loaded(&self, pos: ChunkPos) -> bool {
        self.chunks
            .read_sync(&pos, |_, holder| holder.entity_visibility().is_ticking())
            .unwrap_or(false)
    }

    /// Captures the live state for an exact block-entity owner in an eligible holder.
    ///
    /// Holder data remains the outermost guard; `LevelChunk` then acquires section
    /// and storage reads in the same order as block-state writers.
    pub(crate) fn block_entity_tick_state_if_owned(
        &self,
        holder: &Arc<ChunkHolder>,
        pos: BlockPos,
        expected: &SharedBlockEntity,
    ) -> Option<BlockStateId> {
        let chunk_pos = ChunkPos::from_block_pos(pos);
        let active = self
            .chunks
            .read_sync(&chunk_pos, |_, current| Arc::ptr_eq(current, holder))
            .unwrap_or(false);
        if !active
            || !is_block_ticking(holder.simulation_level())
            || !holder.ticking_readiness_snapshot().is_block_ticking()
        {
            return None;
        }

        let chunk = holder.try_chunk(ChunkStatus::Full)?;
        chunk
            .as_full()?
            .block_entity_tick_state_if_owned(pos, expected)
    }

    /// Re-selects one ticker from the live state without retaining a chunk guard
    /// across behavior selection or manager registration.
    pub(crate) fn reconcile_block_entity_ticker(&self, holder: &Arc<ChunkHolder>, pos: BlockPos) {
        let world = self.world_gen_context.world();
        let chunk_pos = ChunkPos::from_block_pos(pos);
        let active = self
            .chunks
            .read_sync(&chunk_pos, |_, current| Arc::ptr_eq(current, holder))
            .unwrap_or(false);
        if !active {
            world.block_entity_tickers().remove(holder, pos);
            return;
        }

        let target = {
            let Some(chunk) = holder.try_chunk(ChunkStatus::Full) else {
                world.block_entity_tickers().remove(holder, pos);
                return;
            };
            chunk
                .as_full()
                .and_then(|chunk| chunk.block_entity_tick_target(pos))
        };
        let Some((state, block_entity)) = target else {
            world.block_entity_tickers().remove(holder, pos);
            return;
        };
        if block_entity.is_removed() {
            world.block_entity_tickers().remove(holder, pos);
            return;
        }

        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        let ticker = behavior.get_block_entity_ticker(&world, state, block_entity.get_type());
        let ticker = ticker.filter(|ticker| {
            let valid = ticker.accepts(block_entity.get_type());
            if !valid {
                tracing::error!(
                    block = %state.get_block().key,
                    block_entity_type = %block_entity.get_type().key,
                    ?pos,
                    "Block behavior returned a ticker for the wrong block-entity type"
                );
            }
            valid
        });
        world
            .block_entity_tickers()
            .reconcile(holder, block_entity, ticker);
    }

    pub(crate) fn activate_block_entities<'a>(
        &self,
        holders: impl IntoIterator<Item = &'a Arc<ChunkHolder>>,
    ) {
        for holder in holders {
            if !holder.load_level().is_some_and(is_full)
                || !self
                    .chunks
                    .read_sync(&holder.get_pos(), |_, active| Arc::ptr_eq(active, holder))
                    .unwrap_or(false)
                || !holder.is_full_status_initialized()
                || holder.persisted_status() != Some(ChunkStatus::Full)
            {
                continue;
            }
            let batch = {
                let Some(chunk) = holder.try_chunk(ChunkStatus::Full) else {
                    continue;
                };
                chunk
                    .as_full()
                    .and_then(|chunk| chunk.prepare_block_entity_activation(holder))
            };
            let Some(batch) = batch else {
                continue;
            };
            for block_entity in batch.lifecycle_dispatchers {
                block_entity.dispatch_lifecycle_events();
            }
            for pos in batch.positions {
                {
                    let Some(chunk) = holder.try_chunk(ChunkStatus::Full) else {
                        break;
                    };
                    let Some(chunk) = chunk.as_full() else {
                        break;
                    };
                    chunk.reconcile_block_entity_game_event_listener(pos);
                }
                self.reconcile_block_entity_ticker(holder, pos);
            }
        }
    }

    fn finish_block_entity_unloads(&self) {
        let finalized = mem::take(&mut *self.finalized_block_entity_unloads.lock());
        if finalized.is_empty() {
            return;
        }

        let world = self.world_gen_context.world();
        for mut unload in finalized {
            let mut lifecycle_dispatchers = unload
                .holder
                .try_chunk(ChunkStatus::Empty)
                .and_then(|chunk| {
                    chunk
                        .as_full()
                        .map(|chunk| chunk.deactivate_block_entities(&unload.holder))
                })
                .unwrap_or_default();
            world
                .block_entity_tickers()
                .remove_positions(&unload.holder, &unload.positions);
            lifecycle_dispatchers.append(&mut unload.lifecycle_dispatchers);
            for block_entity in lifecycle_dispatchers {
                block_entity.dispatch_lifecycle_events();
            }
        }
    }

    /// Places (or refreshes) the timeout ticket that keeps a thrown ender pearl's
    /// chunk loaded and ticking while it flies.
    ///
    /// Mirrors vanilla `ServerPlayer.placeEnderPearlTicket` →
    /// `chunkSource.addTicketWithRadius(ENDER_PEARL, chunk, 2)`. Re-placing the
    /// same ticket resets its countdown rather than stacking duplicates.
    // TODO: vanilla's ENDER_PEARL ticket also sets FLAG_KEEP_DIMENSION_ACTIVE
    // (`resetEmptyTime`/`shouldKeepDimensionActive`); SteelMC has no idle-dimension
    // unload concept yet, so that flag has no analog here.
    pub fn place_ender_pearl_ticket(&self, chunk: ChunkPos) {
        let mut timed_tickets = self.timed_chunk_tickets.lock();
        let ticket = timed_tickets.add_ender_pearl_ticket(chunk);
        if let Some(ticket) = ticket {
            self.add_chunk_ticket(chunk, ticket);
        }
    }
}

#[cfg(test)]
mod tests;
