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
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::dimension_type::DimensionTypeRef;
use steel_utils::{BlockPos, ChunkPos, SectionPos, locks::SyncMutex};
use tokio::runtime::Runtime;
use tokio::sync::Notify;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::instrument;

use crate::behavior::BlockStateBehaviorExt;
use crate::behavior::{BLOCK_BEHAVIORS, FLUID_BEHAVIORS};
use crate::chunk::chunk_holder::ChunkHolder;
use crate::chunk::chunk_ticket_manager::{
    ChunkTicket, ChunkTicketLevel, ChunkTicketManager, LevelChange, generation_status, is_ticked,
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
};
use crate::chunk_saver::ChunkStorage;
use crate::player::connection::NetworkConnection;
use crate::world::World;
use crate::world::tick_scheduler::{BlockTick, FluidTick};
use crate::worldgen::{ChunkGeneratorType, WorldGenContext};
use crate::{entity::Entity, player::Player};

const GENERATION_THREAD_MULTIPLE: usize = 2;

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
    /// Number of chunks that were ticked.
    pub tickable_count: usize,
    /// Total number of loaded chunks.
    pub total_chunks: usize,
}

/// Timing information for the chunk scheduling tick operations.
#[derive(Debug, Default)]
pub struct ChunkMapSchedulingTimings {
    /// Time spent processing ticket updates.
    pub ticket_updates: Duration,
    /// Time spent creating/updating chunk holders.
    pub holder_creation: Duration,
    /// Time spent scheduling generation tasks.
    pub schedule_generation: Duration,
    /// Number of holders scheduled for generation.
    pub scheduled_count: usize,
    /// Time spent spawning generation tasks.
    pub run_generation: Duration,
    /// Time spent processing chunk unloads.
    pub process_unloads: Duration,
}

#[derive(Debug, Default)]
struct PendingLightUpdates {
    chunks: FxHashMap<ChunkPos, PendingChunkLightUpdates>,
    queued_chunks: Vec<ChunkPos>,
}

impl PendingLightUpdates {
    fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    fn next_center(&self) -> Option<ChunkPos> {
        self.queued_chunks
            .iter()
            .copied()
            .find(|chunk_pos| self.chunks.contains_key(chunk_pos))
    }

    fn next_center_touching_chunk(&self, chunk_pos: ChunkPos) -> Option<ChunkPos> {
        self.queued_chunks.iter().copied().find(|center| {
            self.chunks.contains_key(center) && light_update_window_contains(*center, chunk_pos)
        })
    }

    fn queue_change(
        &mut self,
        chunk_pos: ChunkPos,
        pos: BlockPos,
        check_block: bool,
        empty_section_change: Option<LightSectionEmptinessChange>,
    ) {
        if !self.chunks.contains_key(&chunk_pos) {
            self.queued_chunks.push(chunk_pos);
        }

        let task = self.chunks.entry(chunk_pos).or_default();
        if check_block {
            task.changed_positions.insert(pos);
        }
        if let Some(change) = empty_section_change {
            task.changed_sections
                .insert(change.section_pos, change.empty);
        }
    }

    fn drain(&mut self) -> Vec<(ChunkPos, PendingChunkLightUpdates)> {
        let mut chunks = mem::take(&mut self.chunks);
        let queued_chunks = mem::take(&mut self.queued_chunks);
        queued_chunks
            .into_iter()
            .filter_map(|chunk_pos| chunks.remove(&chunk_pos).map(|task| (chunk_pos, task)))
            .collect()
    }

    fn drain_center(&mut self, chunk_pos: ChunkPos) -> Option<PendingChunkLightUpdates> {
        let task = self.chunks.remove(&chunk_pos)?;
        self.queued_chunks.retain(|&queued| queued != chunk_pos);
        Some(task)
    }

    fn prepend_drained(&mut self, tasks: Vec<(ChunkPos, PendingChunkLightUpdates)>) {
        let previous_queued_chunks = mem::take(&mut self.queued_chunks);
        let mut prepended_chunks = FxHashSet::default();

        for (chunk_pos, task) in tasks {
            if task.is_empty() {
                continue;
            }

            if let Some(existing) = self.chunks.get_mut(&chunk_pos) {
                existing.merge_older(task);
            } else {
                self.chunks.insert(chunk_pos, task);
            }

            if prepended_chunks.insert(chunk_pos) {
                self.queued_chunks.push(chunk_pos);
            }
        }

        for chunk_pos in previous_queued_chunks {
            if !prepended_chunks.contains(&chunk_pos) {
                self.queued_chunks.push(chunk_pos);
            }
        }
    }
}

#[derive(Debug, Default)]
struct LightUpdateState {
    pending: PendingLightUpdates,
    in_flight_centers: FxHashMap<ChunkPos, usize>,
}

impl LightUpdateState {
    #[cfg(test)]
    fn is_idle(&self) -> bool {
        self.pending.is_empty() && self.in_flight_centers.is_empty()
    }

    fn has_in_flight_updates(&self) -> bool {
        !self.in_flight_centers.is_empty()
    }

    fn has_in_flight_update_touching_chunk(&self, chunk_pos: ChunkPos) -> bool {
        self.in_flight_centers
            .keys()
            .copied()
            .any(|center| light_update_window_contains(center, chunk_pos))
    }

    fn track_in_flight(&mut self, centers: &[ChunkPos]) {
        for &center in centers {
            *self.in_flight_centers.entry(center).or_default() += 1;
        }
    }

    fn finish_in_flight(&mut self, centers: &[ChunkPos]) {
        for center in centers {
            let Some(count) = self.in_flight_centers.get_mut(center) else {
                debug_assert!(false, "in-flight light update counter underflow");
                continue;
            };
            *count -= 1;
            if *count == 0 {
                self.in_flight_centers.remove(center);
            }
        }
    }

    fn touches_chunk(&self, chunk_pos: ChunkPos) -> bool {
        self.pending
            .chunks
            .keys()
            .copied()
            .chain(self.in_flight_centers.keys().copied())
            .any(|center| light_update_window_contains(center, chunk_pos))
    }
}

struct InFlightLightUpdates<'a> {
    centers: Vec<ChunkPos>,
    light_updates: &'a SyncMutex<LightUpdateState>,
    progress_notify: &'a Notify,
}

impl Drop for InFlightLightUpdates<'_> {
    fn drop(&mut self) {
        {
            let mut light_updates = self.light_updates.lock();
            light_updates.finish_in_flight(&self.centers);
        }
        self.progress_notify.notify_waiters();
    }
}

const fn light_update_window_contains(center: ChunkPos, chunk_pos: ChunkPos) -> bool {
    let dx = center.0.x.abs_diff(chunk_pos.0.x);
    let dz = center.0.y.abs_diff(chunk_pos.0.y);
    dx <= LIGHT_CACHE_RADIUS as u32 && dz <= LIGHT_CACHE_RADIUS as u32
}

#[derive(Debug, Default)]
struct PendingChunkLightUpdates {
    changed_positions: FxHashSet<BlockPos>,
    changed_sections: FxHashMap<SectionPos, bool>,
}

impl PendingChunkLightUpdates {
    fn is_empty(&self) -> bool {
        self.changed_positions.is_empty() && self.changed_sections.is_empty()
    }

    fn merge_older(&mut self, older: Self) {
        self.changed_positions.extend(older.changed_positions);
        for (section_pos, empty) in older.changed_sections {
            self.changed_sections.entry(section_pos).or_insert(empty);
        }
    }

    fn empty_section_changes(&self) -> Vec<LightSectionEmptinessChange> {
        let mut changes = self
            .changed_sections
            .iter()
            .map(|(&section_pos, &empty)| LightSectionEmptinessChange { section_pos, empty })
            .collect::<Vec<_>>();
        changes.sort_by(|left, right| {
            left.section_pos
                .x()
                .cmp(&right.section_pos.x())
                .then_with(|| left.section_pos.z().cmp(&right.section_pos.z()))
                .then_with(|| right.section_pos.y().cmp(&left.section_pos.y()))
        });
        changes
    }
}

/// A map of chunks managing their state, loading, and generation.
pub struct ChunkMap {
    /// Map of active chunks.
    pub chunks: scc::HashMap<ChunkPos, Arc<ChunkHolder>, FxBuildHasher>,
    /// Map of chunks currently being unloaded.
    pub unloading_chunks: scc::HashMap<ChunkPos, Arc<ChunkHolder>, FxBuildHasher>,
    /// Queue of pending generation tasks.
    pub pending_generation_tasks: SyncMutex<Vec<Arc<ChunkGenerationTask>>>,
    /// Tracker for background generation tasks.
    pub task_tracker: TaskTracker,
    /// Manager for chunk distances and tickets.
    pub chunk_tickets: SyncMutex<ChunkTicketManager>,
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
    /// Last length of `tickable_chunks` to pre-allocate with appropriate capacity.
    last_tickable_len: AtomicUsize,
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
        Self {
            chunks: scc::HashMap::default(),
            unloading_chunks: scc::HashMap::default(),
            pending_generation_tasks: SyncMutex::new(Vec::new()),
            task_tracker: TaskTracker::new(),
            chunk_tickets: SyncMutex::new(ChunkTicketManager::new()),
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
            last_tickable_len: AtomicUsize::new(0),
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
        let chunk_holder = self.chunks.read_sync(&pos, |_, chunk| chunk.clone())?;
        let guard = chunk_holder.try_chunk(status)?;
        Some(f(&guard))
    }

    /// Loads full chunks in a square radius, runs `f`, then removes the temporary ticket.
    pub async fn with_full_chunks_in_radius<F, R>(
        self: &Arc<Self>,
        center: ChunkPos,
        radius: u8,
        f: F,
    ) -> Option<R>
    where
        F: FnOnce() -> R,
    {
        let ticket = ChunkTicket::full_chunks(radius);

        self.chunk_tickets.lock().add_ticket(center, ticket);
        let radius = i32::from(radius);

        loop {
            self.tick_scheduling();
            if self.full_square_is_ready(center, radius) {
                break;
            }

            if self.cancel_token.is_cancelled() {
                self.chunk_tickets.lock().remove_ticket(center, ticket);
                self.tick_scheduling();
                return None;
            }

            sleep(Duration::from_millis(10)).await;
        }

        let result = f();
        self.chunk_tickets.lock().remove_ticket(center, ticket);
        self.tick_scheduling();

        Some(result)
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

    /// Records a block change at the given position.
    /// This marks the chunk as having pending changes to broadcast.
    pub fn block_changed(&self, pos: BlockPos) {
        let chunk_pos = ChunkPos::new(
            SectionPos::block_to_section_coord(pos.0.x),
            SectionPos::block_to_section_coord(pos.0.z),
        );

        if let Some(holder) = self.chunks.read_sync(&chunk_pos, |_, h| h.clone())
            && holder.block_changed(pos)
        {
            // First change for this chunk - add to broadcast list
            self.chunks_to_broadcast.lock().push(holder);
        }
    }

    /// Marks client-visible chunk packet content as changed.
    pub fn packet_content_changed(&self, chunk_pos: ChunkPos) {
        if let Some(holder) = self.chunks.read_sync(&chunk_pos, |_, h| Arc::clone(h)) {
            holder.mark_packet_content_changed();
        }
    }

    /// Records a light-section change at the given position.
    pub fn light_changed(&self, layer: LightLayer, section_pos: SectionPos) {
        let chunk_pos = ChunkPos::new(section_pos.x(), section_pos.z());

        if let Some(holder) = self.chunks.read_sync(&chunk_pos, |_, h| Arc::clone(h)) {
            if holder.light_changed(layer, section_pos) {
                self.chunks_to_broadcast.lock().push(holder);
            }
            return;
        }

        if let Some(holder) = self
            .unloading_chunks
            .read_sync(&chunk_pos, |_, h| Arc::clone(h))
        {
            holder.mark_light_section_dirty(section_pos);
        }
    }

    /// Queues a block or section light change for the next light propagation drain.
    pub fn queue_light_change(
        &self,
        pos: BlockPos,
        check_block: bool,
        empty_section_change: Option<LightSectionEmptinessChange>,
    ) {
        if !check_block && empty_section_change.is_none() {
            return;
        }

        let chunk_pos = ChunkPos::new(
            SectionPos::block_to_section_coord(pos.0.x),
            SectionPos::block_to_section_coord(pos.0.z),
        );

        let mut light_updates = self.light_updates.lock();
        if !self.light_update_center_is_available(chunk_pos) {
            return;
        }

        light_updates
            .pending
            .queue_change(chunk_pos, pos, check_block, empty_section_change);
    }

    /// Drains all queued light updates and runs one scoped propagation per changed chunk.
    pub fn propagate_queued_light_changes(&self) {
        let Some((tasks, in_flight_updates)) = self.drain_pending_light_updates() else {
            return;
        };

        let mut blocked_tasks = Vec::new();
        for (center, task) in tasks {
            if task.is_empty() {
                continue;
            }
            let Some(light_work_window_reservation) =
                self.light_work_window_gate.try_reserve_centered(center)
            else {
                blocked_tasks.push((center, task));
                continue;
            };

            self.propagate_queued_light_change(center, task);
            drop(light_work_window_reservation);
        }

        if !blocked_tasks.is_empty() {
            self.light_updates
                .lock()
                .pending
                .prepend_drained(blocked_tasks);
        }
        drop(in_flight_updates);
    }

    async fn flush_queued_light_changes_for_save(&self) {
        loop {
            let Some(center) = self.next_pending_light_update_center() else {
                if !self.has_in_flight_light_updates() {
                    return;
                }
                self.wait_for_in_flight_light_updates().await;
                continue;
            };

            let light_work_window_reservation =
                self.light_work_window_gate.reserve_centered(center).await;

            let Some((task, in_flight_updates)) =
                self.drain_pending_light_update_for_center(center)
            else {
                drop(light_work_window_reservation);
                continue;
            };

            if task.is_empty() {
                drop(light_work_window_reservation);
                drop(in_flight_updates);
                continue;
            }

            self.propagate_queued_light_change(center, task);
            drop(light_work_window_reservation);
            drop(in_flight_updates);
        }
    }

    fn drain_pending_light_updates(
        &self,
    ) -> Option<(
        Vec<(ChunkPos, PendingChunkLightUpdates)>,
        InFlightLightUpdates<'_>,
    )> {
        let mut light_updates = self.light_updates.lock();
        if light_updates.pending.is_empty() {
            return None;
        }
        let tasks = light_updates.pending.drain();
        let centers = tasks
            .iter()
            .map(|(chunk_pos, _)| *chunk_pos)
            .collect::<Vec<_>>();
        let in_flight = self.track_in_flight_light_updates(&mut light_updates, centers);
        Some((tasks, in_flight))
    }

    fn next_pending_light_update_center(&self) -> Option<ChunkPos> {
        self.light_updates.lock().pending.next_center()
    }

    fn next_pending_light_update_center_touching_chunk(
        &self,
        chunk_pos: ChunkPos,
    ) -> Option<ChunkPos> {
        self.light_updates
            .lock()
            .pending
            .next_center_touching_chunk(chunk_pos)
    }

    fn drain_pending_light_update_for_center(
        &self,
        center: ChunkPos,
    ) -> Option<(PendingChunkLightUpdates, InFlightLightUpdates<'_>)> {
        let mut light_updates = self.light_updates.lock();
        let task = light_updates.pending.drain_center(center)?;
        let in_flight = self.track_in_flight_light_updates(&mut light_updates, vec![center]);
        Some((task, in_flight))
    }

    fn track_in_flight_light_updates(
        &self,
        light_updates: &mut LightUpdateState,
        centers: Vec<ChunkPos>,
    ) -> InFlightLightUpdates<'_> {
        light_updates.track_in_flight(&centers);
        InFlightLightUpdates {
            centers,
            light_updates: &self.light_updates,
            progress_notify: &self.light_updates_progress_notify,
        }
    }

    fn has_in_flight_light_updates(&self) -> bool {
        self.light_updates.lock().has_in_flight_updates()
    }

    fn has_in_flight_light_update_touching_chunk(&self, chunk_pos: ChunkPos) -> bool {
        self.light_updates
            .lock()
            .has_in_flight_update_touching_chunk(chunk_pos)
    }

    async fn wait_for_in_flight_light_updates(&self) {
        loop {
            if !self.has_in_flight_light_updates() {
                return;
            }

            let progress = self.light_updates_progress_notify.notified();
            if !self.has_in_flight_light_updates() {
                return;
            }
            progress.await;
        }
    }

    async fn wait_for_in_flight_light_update_touching_chunk(&self, chunk_pos: ChunkPos) {
        loop {
            if !self.has_in_flight_light_update_touching_chunk(chunk_pos) {
                return;
            }

            let progress = self.light_updates_progress_notify.notified();
            if !self.has_in_flight_light_update_touching_chunk(chunk_pos) {
                return;
            }
            progress.await;
        }
    }

    async fn flush_queued_light_changes_touching_chunk_for_save(&self, chunk_pos: ChunkPos) {
        loop {
            let Some(center) = self.next_pending_light_update_center_touching_chunk(chunk_pos)
            else {
                if !self.has_in_flight_light_update_touching_chunk(chunk_pos) {
                    return;
                }
                self.wait_for_in_flight_light_update_touching_chunk(chunk_pos)
                    .await;
                continue;
            };

            let light_work_window_reservation =
                self.light_work_window_gate.reserve_centered(center).await;

            let Some((task, in_flight_updates)) =
                self.drain_pending_light_update_for_center(center)
            else {
                drop(light_work_window_reservation);
                continue;
            };

            if task.is_empty() {
                drop(light_work_window_reservation);
                drop(in_flight_updates);
                continue;
            }

            self.propagate_queued_light_change(center, task);
            drop(light_work_window_reservation);
            drop(in_flight_updates);
        }
    }

    #[cfg(test)]
    fn has_pending_light_updates(&self) -> bool {
        !self.light_updates.lock().is_idle()
    }

    #[cfg(test)]
    fn light_update_touches_chunk(&self, chunk_pos: ChunkPos) -> bool {
        self.light_updates.lock().touches_chunk(chunk_pos)
    }

    fn light_update_center_is_available(&self, center: ChunkPos) -> bool {
        self.light_update_holder(center)
            .is_some_and(|holder| holder.try_chunk(ChunkStatus::Light).is_some())
    }

    fn light_update_holder(&self, chunk_pos: ChunkPos) -> Option<Arc<ChunkHolder>> {
        self.chunks
            .read_sync(&chunk_pos, |_, holder| Arc::clone(holder))
            .or_else(|| {
                self.unloading_chunks
                    .read_sync(&chunk_pos, |_, holder| Arc::clone(holder))
            })
    }

    fn propagate_queued_light_change(&self, center: ChunkPos, task: PendingChunkLightUpdates) {
        let Some(workset) = self.light_workset_for_change(center) else {
            log::warn!("Failed to set up light workset for queued light update at {center:?}");
            return;
        };

        let empty_sections = task.empty_section_changes();
        let positions = task.changed_positions.into_iter().collect::<Vec<_>>();
        let world = self.world_gen_context.world();

        if world.dimension_type.has_skylight {
            match propagate_sky_light_changes_with_empty_sections(
                &workset,
                positions.iter().copied(),
                empty_sections.iter().copied(),
            ) {
                Ok(result) => {
                    for section_pos in result.updated_sections {
                        self.light_changed(LightLayer::Sky, section_pos);
                    }
                }
                Err(error) => {
                    log::warn!(
                        "Failed to propagate queued sky-light change for {center:?}: {error:?}"
                    );
                }
            }
        }

        let Ok(result) =
            propagate_block_light_changes_with_empty_sections(&workset, positions, empty_sections)
        else {
            log::warn!("Failed to propagate queued block-light change for {center:?}");
            return;
        };

        for section_pos in result.updated_sections {
            self.light_changed(LightLayer::Block, section_pos);
        }
    }

    fn light_workset_for_change(&self, center: ChunkPos) -> Option<LightWorkset> {
        let Ok(range) = LightSectionRange::from_world_height(
            self.world_gen_context.min_y(),
            self.world_gen_context.height(),
        ) else {
            return None;
        };

        let layout = LightCacheLayout::new(center, range);
        LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Full,
            true,
            |chunk_pos| {
                let holder = self.light_update_holder(chunk_pos)?;
                drop(holder.try_chunk(ChunkStatus::Light)?);
                Some(holder)
            },
            |_| true,
        )
        .ok()
    }

    /// Broadcasts all pending block and light changes to nearby players.
    #[expect(
        clippy::too_many_lines,
        reason = "broadcasting block and light packets is one ordered publish workflow"
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
        let mut deferred_holders = Vec::new();

        for holder in holders {
            let chunk_pos = holder.get_pos();
            if self.light_updates.lock().touches_chunk(chunk_pos) {
                deferred_holders.push(holder);
                continue;
            }

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

                    tracing::debug!(
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

                    tracing::debug!(
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
                }
            }
        }

        if !deferred_holders.is_empty() {
            self.chunks_to_broadcast.lock().extend(deferred_holders);
        }
    }

    /// Schedules a new generation task.
    #[inline]
    #[instrument(level = "trace", skip(self), fields(chunk = ?pos, target = ?target_status))]
    pub(crate) fn schedule_generation_task_b(
        self: &Arc<Self>,
        target_status: ChunkStatus,
        pos: ChunkPos,
    ) -> Arc<ChunkGenerationTask> {
        let task = Arc::new(ChunkGenerationTask::new(
            pos,
            target_status,
            self.clone(),
            self.generation_pool.clone(),
            self.cancel_token.child_token(),
        ));
        self.pending_generation_tasks.lock().push(Arc::clone(&task));
        task
    }

    /// Runs queued generation tasks.
    #[instrument(level = "trace", skip(self))]
    pub fn run_generation_tasks_b(&self) {
        if self.generation_refill_stopped.load(Ordering::Acquire) {
            return;
        }

        let mut pending = self.pending_generation_tasks.lock();
        if pending.is_empty() {
            return;
        }

        pending.retain(|task| !task.is_cancelled());
        if pending.is_empty() {
            return;
        }

        let running_tasks = self.running_generation_tasks.load(Ordering::Acquire);
        let max_running_tasks = self.max_running_generation_tasks();
        let available_slots = max_running_tasks.saturating_sub(running_tasks);
        if available_slots == 0 {
            tracing::trace!(
                pending = pending.len(),
                running_tasks,
                max_running_tasks,
                "Generation task cap reached"
            );
            return;
        }

        let task_count = pending.len().min(available_slots);
        if task_count < pending.len() {
            pending.sort_by_cached_key(|task| Self::generation_task_priority(task));
        }

        tracing::trace!(
            task_count,
            pending = pending.len(),
            running_tasks,
            max_running_tasks,
            "Running generation tasks"
        );
        let tasks = pending.drain(..task_count).collect::<Vec<_>>();
        self.running_generation_tasks
            .fetch_add(tasks.len(), Ordering::AcqRel);
        drop(pending); // Release lock before spawning

        for task in tasks {
            let permit = RunningGenerationTaskPermit {
                chunk_map: task.chunk_map.clone(),
            };
            self.task_tracker.spawn_on(
                async move {
                    let _permit = permit;
                    task.run().await;
                },
                self.chunk_runtime.handle(),
            );
        }
    }

    fn max_running_generation_tasks(&self) -> usize {
        self.generation_pool.current_num_threads().max(1) * GENERATION_THREAD_MULTIPLE
    }

    fn generation_task_priority(task: &ChunkGenerationTask) -> GenerationTaskPriority {
        let holder = task.center_holder();
        GenerationTaskPriority::for_levels(holder.load_level(), holder.simulation_level())
    }

    /// Updates scheduling for a chunk based on its new level.
    /// Returns the chunk holder if it is active.
    #[inline]
    pub fn update_chunk_level(
        self: &Arc<Self>,
        pos: ChunkPos,
        new_level: Option<ChunkTicketLevel>,
        new_simulation_level: Option<ChunkTicketLevel>,
    ) -> Option<Arc<ChunkHolder>> {
        // Recover from unloading if possible, else create new holder.
        let chunk_holder =
            if let Some(holder) = self.chunks.read_sync(&pos, |_, holder| holder.clone()) {
                holder
            } else {
                let level = new_level?;

                if let Some(entry) = self.unloading_chunks.remove_sync(&pos) {
                    let _ = self.chunks.insert_sync(pos, entry.1.clone());
                    entry.1
                } else {
                    let holder = Arc::new(ChunkHolder::new(
                        pos,
                        level,
                        new_simulation_level,
                        self.world_gen_context.min_y(),
                        self.world_gen_context.height(),
                    ));
                    let _ = self.chunks.insert_sync(pos, holder.clone());
                    holder
                }
            };

        if let Some(level) = new_level {
            let old = chunk_holder.swap_load_level(level);
            chunk_holder.set_simulation_level(new_simulation_level);
            if old != Some(level) {
                chunk_holder.update_highest_allowed_status(Some(level));
            }
            if chunk_holder.try_chunk(ChunkStatus::Empty).is_some() {
                let world = self.world_gen_context.world();
                world.on_entity_chunk_loaded(pos);
                world.update_entity_chunk_visibility(pos, chunk_holder.entity_visibility());
            }
            Some(chunk_holder)
        } else {
            //log::info!("Unloading chunk at {pos:?}");
            chunk_holder.cancel_generation_task();
            chunk_holder.clear_load_level();
            chunk_holder.set_simulation_level(None);
            chunk_holder.update_highest_allowed_status(None);
            // Wake any await_chunk futures so generation tasks holding refs to
            // this chunk can detect the status is disallowed and exit.
            chunk_holder.wake_all_watchers();

            // Clean up POI data for this chunk column
            let world = self.world_gen_context.world();
            world.on_entity_chunk_unload_start(pos);
            world.poi_storage.lock().remove_chunk(pos);

            // Move to unloading_chunks for deferred unload
            if let Some((_, holder)) = self.chunks.remove_sync(&pos) {
                let _ = self.unloading_chunks.insert_sync(pos, holder);
            }
            None
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
        let mut ready_block_ticks = Vec::new();
        let mut ready_fluid_ticks = Vec::new();

        if tick_count.is_multiple_of(100) {
            tracing::debug!(
                chunks = self.chunks.len(),
                unloading = self.unloading_chunks.len(),
                "Chunk map status"
            );
        }

        if !runs_normally {
            return timings;
        }

        {
            let _span = tracing::trace_span!("collect_tickable").entered();
            let start = Instant::now();
            let mut total_chunks = 0;
            let last_len = self.last_tickable_len.load(Ordering::Relaxed);
            let mut tickable_chunks = Vec::with_capacity(last_len);
            self.chunks.iter_sync(|_, holder| {
                total_chunks += 1;
                if is_ticked(holder.simulation_level()) {
                    tickable_chunks.push(holder.clone());
                }
                true
            });
            self.last_tickable_len
                .store(tickable_chunks.len(), Ordering::Relaxed);
            timings.collect_tickable = start.elapsed();
            timings.total_chunks = total_chunks;
            timings.tickable_count = tickable_chunks.len();

            if !tickable_chunks.is_empty() {
                let _span = tracing::trace_span!(
                    "tick_chunks",
                    count = tickable_chunks.len(),
                    total_chunks
                )
                .entered();
                let start = Instant::now();
                for holder in &tickable_chunks {
                    if let Some(chunk_guard) = holder.try_chunk(ChunkStatus::Full) {
                        chunk_guard.drain_ready_scheduled_ticks(
                            &mut ready_block_ticks,
                            &mut ready_fluid_ticks,
                        );
                    }
                }
                Self::execute_scheduled_ticks(world, ready_block_ticks, ready_fluid_ticks);
                for holder in &tickable_chunks {
                    if let Some(chunk_guard) = holder.try_chunk(ChunkStatus::Full) {
                        chunk_guard.tick_random_blocks(random_tick_speed);
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
    pub fn tick_block_entities(&self, timings: &mut ChunkMapGameTickTimings, runs_normally: bool) {
        if !runs_normally {
            return;
        }

        let _span = tracing::trace_span!("block_entities").entered();
        let start = Instant::now();
        self.chunks.iter_sync(|_, holder| {
            if is_ticked(holder.simulation_level())
                && let Some(chunk_guard) = holder.try_chunk(ChunkStatus::Full)
            {
                chunk_guard.tick_block_entities();
            }
            true
        });
        timings.tick_block_entities = start.elapsed();
    }

    /// Scheduling tick: processes tickets, creates holders, schedules generation,
    /// runs generation tasks, and processes unloads.
    ///
    /// Runs on its own independent tick loop, separate from the game tick.
    #[instrument(level = "trace", skip(self), name = "chunk_map_scheduling_tick")]
    pub fn tick_scheduling(self: &Arc<Self>) -> ChunkMapSchedulingTimings {
        let mut timings = ChunkMapSchedulingTimings::default();

        // Only hold the ticket lock for run_all_updates — holder creation and
        // generation scheduling don't need it, and holding it blocks
        // update_player_status on the game tick.
        let changes: Vec<LevelChange> = {
            let _span = tracing::trace_span!("ticket_updates").entered();
            let start = Instant::now();
            let mut ct = self.chunk_tickets.lock();
            let result = ct.run_all_updates().to_vec();
            timings.ticket_updates = start.elapsed();
            result
        };

        let holders_to_schedule: Vec<_> = {
            let _span = tracing::trace_span!("holder_creation").entered();
            let start = Instant::now();
            let result = changes
                .iter()
                .filter_map(|change| {
                    self.update_chunk_level(
                        change.pos,
                        change.new_level,
                        change.new_simulation_level,
                    )
                    .map(|holder| (holder, change.new_level))
                })
                .collect();
            timings.holder_creation = start.elapsed();
            result
        };

        {
            let _span = tracing::trace_span!("schedule_generation").entered();
            let start = Instant::now();
            let scheduled_count = holders_to_schedule
                .iter()
                .filter_map(|(holder, level)| {
                    let status = generation_status(*level)?;
                    holder
                        .schedule_chunk_generation_task_b(status, self)
                        .then_some(())
                })
                .count();
            timings.schedule_generation = start.elapsed();
            timings.scheduled_count = scheduled_count;
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
            self.process_unloads();
            timings.process_unloads = start.elapsed();
        }

        timings
    }

    /// Returns full chunks whose simulation level currently allows entity ticks.
    pub fn tickable_full_chunk_positions(&self) -> Vec<ChunkPos> {
        let mut chunks = Vec::new();
        self.chunks.iter_sync(|_, holder| {
            if is_ticked(holder.simulation_level()) && holder.try_chunk(ChunkStatus::Full).is_some()
            {
                chunks.push(holder.get_pos());
            }
            true
        });
        chunks
    }

    /// Sorts and executes all ready scheduled ticks, calling block/fluid behavior callbacks.
    fn execute_scheduled_ticks(
        world: &Arc<World>,
        mut ready_block_ticks: Vec<BlockTick>,
        mut ready_fluid_ticks: Vec<FluidTick>,
    ) {
        const MAX_TICKS: usize = usize::MAX; // Vanilla uses 65_536, the lion does not concern himself with vanilla hotpatching

        if !ready_block_ticks.is_empty() {
            ready_block_ticks.sort_by(|a, b| {
                a.priority
                    .cmp(&b.priority)
                    .then_with(|| a.sub_tick_order.cmp(&b.sub_tick_order))
            });

            let block_behaviors = &*BLOCK_BEHAVIORS;
            for tick in ready_block_ticks.iter().take(MAX_TICKS) {
                let state = world.get_block_state(tick.pos);
                if state.get_block() != tick.tick_type {
                    continue;
                }
                block_behaviors
                    .get_behavior(tick.tick_type)
                    .tick(state, world, tick.pos);
            }
        }

        if !ready_fluid_ticks.is_empty() {
            ready_fluid_ticks.sort_by(|a, b| {
                a.priority
                    .cmp(&b.priority)
                    .then_with(|| a.sub_tick_order.cmp(&b.sub_tick_order))
            });

            let fluid_behaviors = &*FLUID_BEHAVIORS;
            for tick in ready_fluid_ticks.iter().take(MAX_TICKS) {
                let state = world.get_block_state(tick.pos);
                let fluid_state = state.get_fluid_state();

                // Only execute if the fluid at this location still matches the scheduled tick
                if fluid_state.fluid_id != tick.tick_type {
                    continue;
                }

                fluid_behaviors
                    .get_behavior(tick.tick_type)
                    .tick(world, tick.pos);
            }
        }
    }

    /// Saves a chunk to disk. Does not remove from `unloading_chunks`.
    #[instrument(level = "trace", skip(self, chunk_holder), fields(chunk = ?chunk_holder.get_pos()))]
    async fn save_chunk(&self, chunk_holder: &Arc<ChunkHolder>) {
        let chunk_pos = chunk_holder.get_pos();
        self.flush_queued_light_changes_touching_chunk_for_save(chunk_pos)
            .await;

        // Prepare chunk data while holding the lock, then release before async I/O
        let prepared = {
            let Some(chunk_guard) = chunk_holder.try_chunk(ChunkStatus::StructureStarts) else {
                // Vanilla only persists chunks once they reach StructureStarts.
                // Runtime entities in lower-status chunks are an accepted loss
                // on unload/shutdown until those chunks cross that boundary.
                return;
            };

            let status = chunk_holder
                .persisted_status()
                .expect("The check above confirmed it exists");

            let world = self.world_gen_context.world();
            let runtime_entities = world
                .entity_manager()
                .get_saveable_entities_for_chunk(chunk_pos);
            let force = world.entity_manager().has_save_pending_for_chunk(chunk_pos);
            let dirty = chunk_guard.take_dirty();
            let prepared = if dirty || force {
                ChunkStorage::prepare_chunk_save(&chunk_guard, &runtime_entities, true)
            } else {
                None
            };

            if prepared.is_none() && dirty {
                chunk_guard.mark_dirty();
            }

            (prepared, status)
        }; // chunk_guard dropped here

        let (prepared, status) = prepared;

        // Save chunk data if dirty
        if let Some(mut prepared) = prepared {
            let handled_runtime_entity_ids = mem::take(&mut prepared.handled_runtime_entity_ids);
            let world = self.world_gen_context.world();
            match self.storage.save_chunk_data(prepared, status).await {
                Ok(true) => world
                    .entity_manager()
                    .on_chunk_saved(chunk_pos, &handled_runtime_entity_ids),
                Ok(false) => Self::mark_chunk_dirty_for_save_retry(chunk_holder),
                Err(e) => {
                    tracing::error!("Error saving chunk: {e}");
                    Self::mark_chunk_dirty_for_save_retry(chunk_holder);
                }
            }
        }
    }

    fn mark_chunk_dirty_for_save_retry(chunk_holder: &ChunkHolder) {
        let Some(chunk) = chunk_holder.try_chunk(ChunkStatus::StructureStarts) else {
            return;
        };
        chunk.mark_dirty();
    }

    /// Processes chunks that are pending unload.
    ///
    /// Iterates over `unloading_chunks`. For each chunk with `strong_count == 1`:
    /// - If dirty: spawn save task (keep until saved and clean)
    /// - If not dirty: release region handle and remove
    #[instrument(level = "trace", skip(self))]
    pub fn process_unloads(self: &Arc<Self>) {
        self.propagate_queued_light_changes();

        let light_updates = self.light_updates.lock();
        self.unloading_chunks.retain_sync(|pos, holder| {
            if light_updates.touches_chunk(*pos) {
                return true;
            }

            if Arc::strong_count(holder) == 1 {
                // Check if dirty by trying to get chunk access
                let is_dirty = holder
                    .try_chunk(ChunkStatus::StructureStarts)
                    .is_some_and(|chunk| chunk.is_dirty());
                let has_save_pending_entities = self
                    .world_gen_context
                    .world()
                    .entity_manager()
                    .has_save_pending_for_chunk(*pos);

                if is_dirty || has_save_pending_entities {
                    // Save the chunk, keep until next tick when it's clean
                    let holder_clone = holder.clone();
                    let map_clone = self.clone();
                    self.task_tracker.spawn(async move {
                        map_clone.save_chunk(&holder_clone).await;
                    });
                    true // keep until clean
                } else if holder.try_chunk(ChunkStatus::Empty).is_none() {
                    let world = self.world_gen_context.world();
                    world.on_entity_chunk_unload_finalized(*pos);
                    false
                } else {
                    // Clean and no refs - release region handle and remove
                    let pos = *pos;
                    let world = self.world_gen_context.world();
                    world.on_entity_chunk_unload_finalized(pos);
                    let map_clone = self.clone();
                    self.task_tracker.spawn(async move {
                        if let Err(e) = map_clone.storage.release_chunk(pos).await {
                            tracing::error!(?pos, "Error releasing chunk: {e}");
                        }
                    });
                    false // remove
                }
            } else {
                true // keep, still has refs
            }
        });
    }

    /// Updates the player's status in the chunk map.
    pub fn update_player_status(&self, player: &Player) {
        let current_chunk_pos = ChunkPos::from_entity_pos(player.position());
        *player.last_chunk_pos.lock() = current_chunk_pos;
        let view_distance = player.view_distance();

        let new_view = PlayerChunkView::new(current_chunk_pos, view_distance);
        let world = self.world_gen_context.world();
        let mut last_view_guard = player.last_tracking_view.lock();

        if last_view_guard.as_ref() != Some(&new_view) {
            let mut chunk_tickets = self.chunk_tickets.lock();

            let new_ticket = ChunkTicket::player(new_view.view_distance, world.simulation_distance);

            if let Some(last_view) = last_view_guard.as_ref() {
                if last_view.center != new_view.center
                    || last_view.view_distance != new_view.view_distance
                {
                    let old_ticket =
                        ChunkTicket::player(last_view.view_distance, world.simulation_distance);
                    chunk_tickets.remove_ticket(last_view.center, old_ticket);
                    chunk_tickets.add_ticket(new_view.center, new_ticket);

                    player.send_packet(CSetChunkCenter {
                        x: new_view.center.0.x,
                        y: new_view.center.0.y,
                    });
                }

                // Track chunks for PlayerAreaMap update
                let mut added_chunks = Vec::new();
                let mut removed_chunks = Vec::new();

                // We lock here to ensure we have unique access for the duration of the diff
                let mut chunk_sender = player.chunk_sender.lock();
                let connection = &*player.connection;
                PlayerChunkView::difference(
                    last_view,
                    &new_view,
                    |pos, ctx: &mut (&mut _, &mut Vec<_>, &mut Vec<_>)| {
                        ctx.0.mark_chunk_pending_to_send(pos);
                        ctx.1.push(pos);
                    },
                    |pos, ctx: &mut (&mut _, &mut Vec<_>, &mut Vec<_>)| {
                        ctx.0.drop_chunk(connection, pos);
                        ctx.2.push(pos);
                    },
                    &mut (&mut chunk_sender, &mut added_chunks, &mut removed_chunks),
                );
                drop(chunk_sender);

                // Update the player area map with the diff
                world.player_area_map.on_player_view_change(
                    player.id(),
                    &added_chunks,
                    &removed_chunks,
                );
            } else {
                chunk_tickets.add_ticket(new_view.center, new_ticket);

                // Send initial chunk cache center to client
                player.send_packet(CSetChunkCenter {
                    x: new_view.center.0.x,
                    y: new_view.center.0.y,
                });

                let mut chunk_sender = player.chunk_sender.lock();
                new_view.for_each(|pos| {
                    chunk_sender.mark_chunk_pending_to_send(pos);
                });
                drop(chunk_sender);

                // First time - add all chunks in view to player area map
                world.player_area_map.on_player_join(player, &new_view);
            }

            *last_view_guard = Some(new_view);
        }
        drop(last_view_guard);

        // Entity visibility also depends on exact player position, not only
        // chunk-view changes. Vanilla refreshes tracked entities for accepted
        // movement within the same chunk as well.
        let sent_chunks = player.chunk_sender.lock().sent_chunks_snapshot();
        world
            .entity_tracker()
            .update_player(player, &new_view, |chunk| sent_chunks.contains(&chunk));
    }

    /// Removes a player from the chunk map.
    pub fn remove_player(&self, player: &Player) {
        // Okay to lock sync lock here cause it has low contention
        let mut last_view_guard = player.last_tracking_view.lock();
        if let Some(last_view) = last_view_guard.take() {
            drop(last_view_guard);
            let mut chunk_tickets = self.chunk_tickets.lock();
            let world = self.world_gen_context.world();
            let ticket = ChunkTicket::player(last_view.view_distance, world.simulation_distance);
            chunk_tickets.remove_ticket(last_view.center, ticket);
        }
    }

    /// Saves all dirty chunks to disk.
    ///
    /// This method should be called during graceful shutdown to ensure all
    /// modified chunks are persisted. It saves:
    /// 1. All dirty chunks in the active `chunks` map
    /// 2. All chunks pending unload in the `unloading_chunks` map
    /// 3. Closes all region file handles (flushing headers)
    ///
    /// Returns the number of chunks saved.
    #[instrument(level = "info", skip(self), name = "save_all_chunks")]
    pub async fn save_all_chunks(self: &Arc<Self>) -> io::Result<usize> {
        let mut saved_count = 0;

        self.flush_queued_light_changes_for_save().await;

        // Collect all chunks from both maps
        let all_chunks: Vec<Arc<ChunkHolder>> = {
            let mut chunks = Vec::new();
            self.chunks.iter_sync(|_, holder| {
                chunks.push(holder.clone());
                true
            });
            self.unloading_chunks.iter_sync(|_, holder| {
                chunks.push(holder.clone());
                true
            });
            chunks
        };
        let mut covered_chunk_positions = FxHashSet::default();

        tracing::info!(chunk_count = all_chunks.len(), "Saving chunks");

        // Save all chunks that have data
        for holder in &all_chunks {
            let chunk_pos = holder.get_pos();
            let prepared = {
                let Some(chunk) = holder.try_chunk(ChunkStatus::StructureStarts) else {
                    // Matches save_chunk: StructureStarts is the first persisted
                    // chunk status, so lower-status chunks do not own durable
                    // runtime entity data.
                    continue;
                };
                let Some(status) = holder.persisted_status() else {
                    continue;
                };
                let world = self.world_gen_context.world();
                let runtime_entities = world
                    .entity_manager()
                    .get_saveable_entities_for_chunk(chunk_pos);
                let force = world.entity_manager().has_save_pending_for_chunk(chunk_pos);
                let dirty = chunk.take_dirty();
                let prepared = if dirty || force {
                    ChunkStorage::prepare_chunk_save(&chunk, &runtime_entities, true)
                } else {
                    None
                };
                let Some(prepared) = prepared else {
                    if dirty {
                        chunk.mark_dirty();
                    } else if !force {
                        covered_chunk_positions.insert(chunk_pos);
                    }
                    continue;
                };
                (prepared, status)
            };

            let (mut prepared, status) = prepared;
            let handled_runtime_entity_ids = mem::take(&mut prepared.handled_runtime_entity_ids);
            let world = self.world_gen_context.world();
            match self.storage.save_chunk_data(prepared, status).await {
                Ok(true) => {
                    world
                        .entity_manager()
                        .on_chunk_saved(chunk_pos, &handled_runtime_entity_ids);
                    covered_chunk_positions.insert(chunk_pos);
                    saved_count += 1;
                }
                Ok(false) => Self::mark_chunk_dirty_for_save_retry(holder),
                Err(e) => {
                    tracing::error!(chunk = ?holder.get_pos(), "Failed to save chunk: {e}");
                    Self::mark_chunk_dirty_for_save_retry(holder);
                }
            }
        }

        let world = self.world_gen_context.world();
        let covered_chunk_positions = covered_chunk_positions.into_iter().collect::<Vec<_>>();
        let unsaved_entities = world
            .entity_manager()
            .saveable_entities_outside_chunks(&covered_chunk_positions);
        if !unsaved_entities.is_empty() {
            let chunk_count = unsaved_entities
                .iter()
                .map(|entity| entity.chunk)
                .collect::<FxHashSet<_>>()
                .len();
            let sample = unsaved_entities
                .iter()
                .take(16)
                .map(|entity| format!("{}:{}@{:?}", entity.entity_id, entity.uuid, entity.chunk))
                .collect::<Vec<_>>()
                .join(", ");
            tracing::warn!(
                entity_count = unsaved_entities.len(),
                chunk_count,
                sample = %sample,
                "Saveable runtime entities remain in chunks without save holders after chunk save"
            );
        }

        // Close all region files (flushes headers and releases file handles)
        if let Err(e) = self.storage.close_all().await {
            tracing::error!("Failed to close region files: {e}");
        }

        tracing::info!(
            saved_count,
            total_checked = all_chunks.len(),
            "Chunk save complete"
        );

        Ok(saved_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::init_behaviors;
    use crate::chunk::heightmap::ChunkHeightmaps;
    use crate::chunk::level_chunk::LevelChunk;
    use crate::chunk::light::ChunkLightData;
    use crate::chunk::proto_chunk::ProtoChunk;
    use crate::chunk::section::{ChunkSection, Sections};
    use crate::chunk_saver::RamOnlyStorage;
    use crate::world::tick_scheduler::{BlockTickList, FluidTickList};
    use crate::worldgen::EmptyChunkGenerator;
    use steel_registry::{test_support::init_test_registry, vanilla_dimension_types::OVERWORLD};
    use steel_worldgen::structure::{StructureReferenceMap, StructureStartMap};

    #[test]
    fn generation_priority_prefers_simulation_tickets() {
        let normal_strong = GenerationTaskPriority::for_levels(
            Some(ChunkTicketLevel::for_full_chunk_radius(8)),
            None,
        );
        let simulated_weak = GenerationTaskPriority::for_levels(
            Some(ChunkTicketLevel::for_full_chunk_radius(1)),
            Some(ChunkTicketLevel::for_full_chunk_radius(1)),
        );

        assert!(simulated_weak < normal_strong);
    }

    #[test]
    fn generation_priority_orders_simulation_by_simulation_level() {
        let weaker_simulation = GenerationTaskPriority::for_levels(
            Some(ChunkTicketLevel::for_full_chunk_radius(8)),
            Some(ChunkTicketLevel::for_full_chunk_radius(1)),
        );
        let stronger_simulation = GenerationTaskPriority::for_levels(
            Some(ChunkTicketLevel::for_full_chunk_radius(1)),
            Some(ChunkTicketLevel::for_full_chunk_radius(4)),
        );

        assert!(stronger_simulation < weaker_simulation);
    }

    #[test]
    fn generation_priority_orders_normal_by_load_level() {
        let weaker_load = GenerationTaskPriority::for_levels(
            Some(ChunkTicketLevel::for_full_chunk_radius(1)),
            None,
        );
        let stronger_load = GenerationTaskPriority::for_levels(
            Some(ChunkTicketLevel::for_full_chunk_radius(4)),
            None,
        );

        assert!(stronger_load < weaker_load);
    }

    fn test_chunk_map() -> Arc<ChunkMap> {
        init_test_registry();
        init_behaviors();
        Arc::new(ChunkMap::new_with_storage(
            Arc::new(Runtime::new().expect("test runtime should initialize")),
            Weak::new(),
            &OVERWORLD,
            63,
            Arc::new(ChunkStorage::RamOnly(RamOnlyStorage::empty_world())),
            Arc::new(ChunkGeneratorType::Empty(EmptyChunkGenerator::new())),
            Arc::new(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .build()
                    .expect("test generation pool should initialize"),
            ),
        ))
    }

    fn unloaded_light_holder(pos: ChunkPos) -> Arc<ChunkHolder> {
        let proto = ProtoChunk::from_disk(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            pos,
            ChunkStatus::Light,
            0,
            16,
            StructureStartMap::default(),
            StructureReferenceMap::default(),
            None,
            Vec::new(),
            BlockTickList::new(),
            FluidTickList::new(),
            Weak::new(),
            ChunkLightData::for_valid_world_height(0, 16),
        );
        let holder = Arc::new(ChunkHolder::new(
            pos,
            ChunkTicketLevel::FULL_CHUNK,
            Some(ChunkTicketLevel::FULL_CHUNK),
            0,
            16,
        ));
        holder.insert_chunk(ChunkAccess::Proto(proto), ChunkStatus::Light);
        holder
    }

    fn unloaded_full_holder(pos: ChunkPos) -> Arc<ChunkHolder> {
        let chunk = LevelChunk::from_disk(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            pos,
            0,
            16,
            Weak::new(),
            BlockTickList::new(),
            FluidTickList::new(),
            ChunkHeightmaps::new(0, 16),
            StructureStartMap::default(),
            StructureReferenceMap::default(),
            ChunkLightData::for_valid_world_height(0, 16),
        );
        let holder = Arc::new(ChunkHolder::new(
            pos,
            ChunkTicketLevel::FULL_CHUNK,
            Some(ChunkTicketLevel::FULL_CHUNK),
            0,
            16,
        ));
        holder.insert_chunk(ChunkAccess::Full(chunk), ChunkStatus::Full);
        holder
    }

    #[test]
    fn light_update_center_is_available_in_unloading_chunks() {
        let chunk_map = test_chunk_map();
        let pos = ChunkPos::new(2, 3);
        let holder = unloaded_light_holder(pos);
        let _ = chunk_map.unloading_chunks.insert_sync(pos, holder);

        assert!(chunk_map.light_update_center_is_available(pos));
    }

    #[test]
    fn light_changed_marks_unloading_chunk_dirty() {
        let chunk_map = test_chunk_map();
        let pos = ChunkPos::new(2, 3);
        let holder = unloaded_light_holder(pos);
        let _ = chunk_map
            .unloading_chunks
            .insert_sync(pos, Arc::clone(&holder));

        let chunk = holder
            .try_chunk(ChunkStatus::Light)
            .expect("test holder should contain a light-status chunk");
        chunk.clear_dirty();
        drop(chunk);

        chunk_map.light_changed(LightLayer::Block, SectionPos::new(pos.0.x, 0, pos.0.y));

        let chunk = holder
            .try_chunk(ChunkStatus::Light)
            .expect("test holder should still contain a light-status chunk");
        assert!(chunk.is_dirty());
    }

    #[test]
    fn light_changed_does_not_broadcast_unloading_full_chunk() {
        let chunk_map = test_chunk_map();
        let pos = ChunkPos::new(2, 3);
        let holder = unloaded_full_holder(pos);
        let _ = chunk_map
            .unloading_chunks
            .insert_sync(pos, Arc::clone(&holder));

        let chunk = holder
            .try_chunk(ChunkStatus::Full)
            .expect("test holder should contain a full chunk");
        chunk.clear_dirty();
        drop(chunk);

        chunk_map.light_changed(LightLayer::Block, SectionPos::new(pos.0.x, 0, pos.0.y));

        let chunk = holder
            .try_chunk(ChunkStatus::Full)
            .expect("test holder should still contain a full chunk");
        assert!(chunk.is_dirty());
        drop(chunk);

        assert!(chunk_map.chunks_to_broadcast.lock().is_empty());
        assert!(!holder.has_changes_to_broadcast());
    }

    #[test]
    fn save_retry_marks_same_unloading_holder_dirty() {
        let _chunk_map = test_chunk_map();
        let pos = ChunkPos::new(2, 3);
        let holder = unloaded_light_holder(pos);
        let chunk = holder
            .try_chunk(ChunkStatus::Light)
            .expect("test holder should contain a light-status chunk");
        chunk.clear_dirty();
        drop(chunk);

        ChunkMap::mark_chunk_dirty_for_save_retry(&holder);

        let chunk = holder
            .try_chunk(ChunkStatus::Light)
            .expect("test holder should still contain a light-status chunk");
        assert!(chunk.is_dirty());
    }

    #[test]
    fn drained_light_updates_remain_unload_blocking_until_applied() {
        let chunk_map = test_chunk_map();
        let center = ChunkPos::new(0, 0);
        chunk_map.light_updates.lock().pending.queue_change(
            center,
            BlockPos::new(1, 2, 3),
            true,
            None,
        );

        let Some((_tasks, in_flight_updates)) = chunk_map.drain_pending_light_updates() else {
            panic!("queued light update should drain");
        };

        assert!(chunk_map.light_updates.lock().pending.is_empty());
        assert!(chunk_map.has_pending_light_updates());

        drop(in_flight_updates);

        assert!(!chunk_map.has_pending_light_updates());
    }

    #[test]
    fn light_update_unload_barrier_is_limited_to_cache_window() {
        let chunk_map = test_chunk_map();
        let center = ChunkPos::new(0, 0);
        let inside = ChunkPos::new(LIGHT_CACHE_RADIUS, -LIGHT_CACHE_RADIUS);
        let outside = ChunkPos::new(LIGHT_CACHE_RADIUS + 1, 0);
        chunk_map.light_updates.lock().pending.queue_change(
            center,
            BlockPos::new(1, 2, 3),
            true,
            None,
        );

        assert!(chunk_map.light_update_touches_chunk(inside));
        assert!(!chunk_map.light_update_touches_chunk(outside));
    }

    #[test]
    fn drained_light_update_window_remains_unload_blocking_until_applied() {
        let chunk_map = test_chunk_map();
        let center = ChunkPos::new(0, 0);
        let inside = ChunkPos::new(LIGHT_CACHE_RADIUS, 0);
        chunk_map.light_updates.lock().pending.queue_change(
            center,
            BlockPos::new(1, 2, 3),
            true,
            None,
        );

        let Some((_tasks, in_flight_updates)) = chunk_map.drain_pending_light_updates() else {
            panic!("queued light update should drain");
        };

        assert!(chunk_map.light_update_touches_chunk(inside));

        drop(in_flight_updates);

        assert!(!chunk_map.light_update_touches_chunk(inside));
    }

    #[test]
    fn broadcast_changed_chunks_defers_holder_while_light_work_is_blocked() {
        let chunk_map = test_chunk_map();
        let center = ChunkPos::new(0, 0);
        let holder = unloaded_full_holder(center);
        assert!(holder.block_changed(BlockPos::new(1, 2, 3)));
        chunk_map
            .chunks_to_broadcast
            .lock()
            .push(Arc::clone(&holder));
        chunk_map.light_updates.lock().pending.queue_change(
            center,
            BlockPos::new(1, 2, 3),
            true,
            None,
        );
        let Some(_reservation) = chunk_map
            .light_work_window_gate
            .try_reserve_centered(center)
        else {
            panic!("test should reserve the light work window");
        };

        chunk_map.broadcast_changed_chunks();

        assert_eq!(chunk_map.chunks_to_broadcast.lock().len(), 1);
        assert!(holder.has_changes_to_broadcast());
        let changes = holder.take_changed_blocks();
        assert_eq!(changes.len(), 1);
        assert!(chunk_map.light_update_touches_chunk(center));
    }

    #[test]
    fn pending_light_updates_coalesce_changes_by_chunk_in_queue_order() {
        let center = ChunkPos::new(0, 0);
        let east = ChunkPos::new(1, 0);
        let center_block = BlockPos::new(1, 2, 3);
        let center_section = SectionPos::new(0, 0, 0);
        let east_block = BlockPos::new(16, 4, 5);
        let mut pending = PendingLightUpdates::default();

        pending.queue_change(center, center_block, true, None);
        pending.queue_change(
            center,
            center_block,
            false,
            Some(LightSectionEmptinessChange {
                section_pos: center_section,
                empty: false,
            }),
        );
        pending.queue_change(east, east_block, true, None);

        let drained = pending.drain();

        assert!(pending.is_empty());
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].0, center);
        assert_eq!(drained[1].0, east);
        assert!(drained[0].1.changed_positions.contains(&center_block));
        assert_eq!(
            drained[0].1.changed_sections.get(&center_section),
            Some(&false)
        );
        assert!(drained[1].1.changed_positions.contains(&east_block));
    }

    #[test]
    fn pending_light_updates_prepend_blocked_drained_tasks() {
        let center = ChunkPos::new(0, 0);
        let east = ChunkPos::new(1, 0);
        let south = ChunkPos::new(0, 1);
        let center_block = BlockPos::new(1, 2, 3);
        let east_block = BlockPos::new(16, 4, 5);
        let south_block = BlockPos::new(1, 6, 16);
        let mut pending = PendingLightUpdates::default();

        pending.queue_change(south, south_block, true, None);
        pending.prepend_drained(vec![
            (
                center,
                PendingChunkLightUpdates {
                    changed_positions: FxHashSet::from_iter([center_block]),
                    changed_sections: FxHashMap::default(),
                },
            ),
            (
                east,
                PendingChunkLightUpdates {
                    changed_positions: FxHashSet::from_iter([east_block]),
                    changed_sections: FxHashMap::default(),
                },
            ),
        ]);

        let drained = pending.drain();

        assert_eq!(
            drained
                .iter()
                .map(|(chunk_pos, _)| *chunk_pos)
                .collect::<Vec<_>>(),
            vec![center, east, south]
        );
        assert!(drained[0].1.changed_positions.contains(&center_block));
        assert!(drained[1].1.changed_positions.contains(&east_block));
        assert!(drained[2].1.changed_positions.contains(&south_block));
    }

    #[test]
    fn pending_light_updates_merge_requeued_task_with_existing_pending_task() {
        let center = ChunkPos::new(0, 0);
        let old_block = BlockPos::new(1, 2, 3);
        let new_block = BlockPos::new(4, 5, 6);
        let section_pos = SectionPos::new(0, 1, 0);
        let mut pending = PendingLightUpdates::default();

        pending.queue_change(
            center,
            new_block,
            true,
            Some(LightSectionEmptinessChange {
                section_pos,
                empty: false,
            }),
        );
        pending.prepend_drained(vec![(
            center,
            PendingChunkLightUpdates {
                changed_positions: FxHashSet::from_iter([old_block]),
                changed_sections: FxHashMap::from_iter([(section_pos, true)]),
            },
        )]);

        let drained = pending.drain();

        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].0, center);
        assert!(drained[0].1.changed_positions.contains(&old_block));
        assert!(drained[0].1.changed_positions.contains(&new_block));
        assert_eq!(
            drained[0].1.changed_sections.get(&section_pos),
            Some(&false)
        );
    }

    #[test]
    fn pending_chunk_light_updates_sort_empty_section_changes_deterministically() {
        let mut task = PendingChunkLightUpdates::default();
        task.changed_sections.insert(SectionPos::new(0, 1, 0), true);
        task.changed_sections
            .insert(SectionPos::new(0, 3, 0), false);
        task.changed_sections
            .insert(SectionPos::new(0, 2, -1), true);
        task.changed_sections
            .insert(SectionPos::new(-1, 0, 0), false);

        let changes = task.empty_section_changes();

        assert_eq!(
            changes,
            vec![
                LightSectionEmptinessChange {
                    section_pos: SectionPos::new(-1, 0, 0),
                    empty: false,
                },
                LightSectionEmptinessChange {
                    section_pos: SectionPos::new(0, 2, -1),
                    empty: true,
                },
                LightSectionEmptinessChange {
                    section_pos: SectionPos::new(0, 3, 0),
                    empty: false,
                },
                LightSectionEmptinessChange {
                    section_pos: SectionPos::new(0, 1, 0),
                    empty: true,
                },
            ]
        );
    }
}
