//! `ChunkHolder` manages chunk state and asynchronous generation tasks.
use futures::Future;
use parking_lot::RwLockReadGuard;
use rustc_hash::FxHashSet;
use std::fmt::Debug;
use std::mem;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use steel_utils::locks::SyncRwLock;
use steel_utils::{BlockPos, ChunkPos, PackedSectionBlockPos, SectionPos, locks::SyncMutex};
use tokio::sync::{oneshot, watch};
#[cfg(feature = "slow_chunk_gen")]
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

#[cfg(feature = "slow_chunk_gen")]
use std::time::Duration;

/// When `true`, each chunk generation stage sleeps 200 ms after completing.
/// Set by the spawn progress display to make the terminal grid visible.
#[cfg(feature = "slow_chunk_gen")]
pub static SLOW_CHUNK_GEN: AtomicBool = AtomicBool::new(false);

use crate::chunk::chunk_generation_task::{NeighborReady, StaticCache2D};
use crate::chunk::chunk_ticket_manager::{ChunkTicketLevel, generation_status};
use crate::chunk_saver::ChunkStorage;
use crate::world::World;
use crate::worldgen::WorldGenContext;
use crate::{
    ChunkMap,
    chunk::{
        chunk_access::{ChunkAccess, ChunkStatus},
        chunk_generation_task::ChunkGenerationTask,
        chunk_pyramid::ChunkStep,
        level_chunk::{LevelChunk, LevelChunkPromotion},
    },
};

const STATUS_NONE: u8 = u8::MAX;
const NO_TICKET_LEVEL: u8 = u8::MAX;

fn optional_ticket_level_raw(level: Option<ChunkTicketLevel>) -> u8 {
    level.map_or(NO_TICKET_LEVEL, ChunkTicketLevel::raw)
}

const fn optional_ticket_level_from_raw(raw: u8) -> Option<ChunkTicketLevel> {
    if raw == NO_TICKET_LEVEL {
        None
    } else {
        ChunkTicketLevel::new(raw)
    }
}

/// The result of a chunk operation.
pub enum ChunkResult {
    /// The chunk is not loaded.
    Unloaded,
    /// The chunk operation succeeded.
    Ok(ChunkStatus),
}

struct ChunkGuard(SyncRwLock<ChunkAccess>);

impl ChunkGuard {
    pub const fn new(chunk_access: ChunkAccess) -> Self {
        ChunkGuard(SyncRwLock::new(chunk_access))
    }

    pub fn read(&self) -> RwLockReadGuard<'_, ChunkAccess> {
        self.0.read_recursive()
    }

    pub fn with_write<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut ChunkAccess) -> R,
    {
        let mut guard = self.0.write();
        f(&mut guard)
    }
}

/// Holds a chunk in a watch channel, allowing for concurrent access and state tracking.
///
/// NOTICE: It is very important to keep data and `chunk_result` in sync.
///
/// `ChunkResult::Unloaded` -> data is None
///
/// `ChunkResult::Ok(status except Full)` -> data is `Some(ChunkAccess::Proto(ProtoChunk))`
///
/// `ChunkResult::Ok(ChunkStatus::Full)` -> data is `Some(ChunkAccess::Full(LevelChunk))`
pub struct ChunkHolder {
    data: ChunkGuard,
    chunk_result: watch::Receiver<ChunkResult>,
    sender: watch::Sender<ChunkResult>,
    generation_task: SyncMutex<Option<Arc<ChunkGenerationTask>>>,
    generation_task_target: AtomicU8,
    pos: ChunkPos,
    /// The current loading ticket level of the chunk.
    load_level: AtomicU8,
    /// The current simulation ticket level of the chunk.
    simulation_level: AtomicU8,
    /// The highest status that has started work.
    started_work: AtomicUsize,
    /// The highest status that generation is allowed to reach.
    highest_allowed_status: AtomicU8,
    /// The minimum Y coordinate of the world.
    min_y: i32,
    /// The total height of the world.
    height: i32,
    /// Whether any sections have pending block changes.
    has_changed_sections: AtomicBool,
    /// Per-section sets of changed block positions.
    /// Index is `(block_y - min_y) / 16`.
    changed_blocks_per_section: Box<[SyncMutex<FxHashSet<PackedSectionBlockPos>>]>,
}

impl ChunkHolder {
    /// Gets the chunk position.
    pub const fn get_pos(&self) -> ChunkPos {
        self.pos
    }

    /// Gets the minimum Y coordinate of the world.
    pub const fn min_y(&self) -> i32 {
        self.min_y
    }

    /// Gets the total height of the world.
    pub const fn height(&self) -> i32 {
        self.height
    }

    /// Creates a new chunk holder.
    #[must_use]
    pub fn new(
        pos: ChunkPos,
        load_level: ChunkTicketLevel,
        simulation_level: Option<ChunkTicketLevel>,
        min_y: i32,
        height: i32,
    ) -> Self {
        let (sender, receiver) = watch::channel(ChunkResult::Unloaded);
        let highest_allowed_status =
            generation_status(Some(load_level)).map_or(STATUS_NONE, |s| s.get_index() as u8);

        let section_count = (height / 16) as usize;
        let changed_blocks_per_section = (0..section_count)
            .map(|_| SyncMutex::new(FxHashSet::default()))
            .collect::<Box<[_]>>();

        Self {
            data: ChunkGuard::new(ChunkAccess::Unloaded),
            chunk_result: receiver,
            sender,
            generation_task: SyncMutex::new(None),
            generation_task_target: AtomicU8::new(STATUS_NONE),
            pos,
            load_level: AtomicU8::new(load_level.raw()),
            simulation_level: AtomicU8::new(optional_ticket_level_raw(simulation_level)),
            started_work: AtomicUsize::new(usize::MAX),
            highest_allowed_status: AtomicU8::new(highest_allowed_status),
            min_y,
            height,
            has_changed_sections: AtomicBool::new(false),
            changed_blocks_per_section,
        }
    }

    /// Returns the current load ticket level.
    pub fn load_level(&self) -> Option<ChunkTicketLevel> {
        optional_ticket_level_from_raw(self.load_level.load(Ordering::Relaxed))
    }

    /// Stores the current load ticket level and returns the previous level.
    pub fn swap_load_level(&self, level: ChunkTicketLevel) -> Option<ChunkTicketLevel> {
        optional_ticket_level_from_raw(self.load_level.swap(level.raw(), Ordering::Relaxed))
    }

    /// Clears the current load ticket level.
    pub fn clear_load_level(&self) {
        self.load_level.store(NO_TICKET_LEVEL, Ordering::Relaxed);
    }

    /// Returns the current simulation ticket level.
    pub fn simulation_level(&self) -> Option<ChunkTicketLevel> {
        optional_ticket_level_from_raw(self.simulation_level.load(Ordering::Relaxed))
    }

    /// Stores the current simulation ticket level.
    pub fn set_simulation_level(&self, level: Option<ChunkTicketLevel>) {
        self.simulation_level
            .store(optional_ticket_level_raw(level), Ordering::Relaxed);
    }

    /// Updates the highest allowed generation status based on the ticket level.
    pub fn update_highest_allowed_status(&self, ticket_level: Option<ChunkTicketLevel>) {
        let new_status =
            generation_status(ticket_level).map_or(STATUS_NONE, |s| s.get_index() as u8);
        self.highest_allowed_status
            .store(new_status, Ordering::Release);
    }

    /// Records a block change at the given position.
    /// Returns `true` if this is the first change (chunk should be added to broadcast list).
    pub fn block_changed(&self, pos: BlockPos) -> bool {
        let section_index = ((pos.0.y - self.min_y) / 16) as usize;
        if section_index >= self.changed_blocks_per_section.len() {
            return false;
        }

        let had_changes = self.has_changed_sections.swap(true, Ordering::AcqRel);
        let packed = SectionPos::section_relative_pos(pos);
        self.changed_blocks_per_section[section_index]
            .lock()
            .insert(packed);

        !had_changes
    }

    /// Returns whether there are pending block changes to broadcast.
    pub fn has_changes_to_broadcast(&self) -> bool {
        self.has_changed_sections.load(Ordering::Acquire)
    }

    /// Takes all pending block changes, grouped by section index.
    /// Returns a vec of (`section_index`, set of packed positions).
    pub fn take_changed_blocks(&self) -> Vec<(usize, FxHashSet<PackedSectionBlockPos>)> {
        if !self.has_changed_sections.swap(false, Ordering::AcqRel) {
            return Vec::new();
        }

        let mut result = Vec::new();
        for (section_index, section_changes) in self.changed_blocks_per_section.iter().enumerate() {
            let mut guard = section_changes.lock();
            if !guard.is_empty() {
                result.push((section_index, mem::take(&mut *guard)));
            }
        }
        result
    }

    /// Returns the number of sections in this chunk.
    pub fn section_count(&self) -> usize {
        self.changed_blocks_per_section.len()
    }

    /// Checks if the given status is disallowed.
    pub fn is_status_disallowed(&self, status: ChunkStatus) -> bool {
        let allowed = self.highest_allowed_status.load(Ordering::Acquire);
        if allowed == STATUS_NONE {
            return true;
        }
        status.get_index() > allowed as usize
    }

    /// Schedules a generation task for this chunk if needed.
    ///
    /// Returns `true` if a new task was actually scheduled, `false` if the chunk
    /// already has a suitable task or is already at the target status.
    #[inline]
    pub(crate) fn schedule_chunk_generation_task_b(
        &self,
        status: ChunkStatus,
        chunk_map: &Arc<ChunkMap>,
    ) -> bool {
        if self.is_status_disallowed(status) {
            return false;
        }

        if self.try_chunk(status).is_some() {
            return false;
        }

        let status_index = status.get_index() as u8;
        let current_target = self.generation_task_target.load(Ordering::Acquire);
        if current_target != STATUS_NONE && status_index <= current_target {
            return false;
        }

        let task = self.generation_task.lock();

        if task
            .as_ref()
            .is_some_and(|task| status <= task.target_status)
        {
            return false;
        }

        drop(task);
        self.reschedule_chunk_task_b(status, chunk_map);
        true
    }

    /// Reschedules the chunk task to the given status.
    #[inline]
    pub(crate) fn reschedule_chunk_task_b(&self, status: ChunkStatus, chunk_map: &Arc<ChunkMap>) {
        let new_task = chunk_map.schedule_generation_task_b(status, self.pos);
        let mut old_task_guard = self.generation_task.lock();

        let old_task = old_task_guard.replace(new_task);
        self.generation_task_target
            .store(status.get_index() as u8, Ordering::Release);
        drop(old_task_guard);

        if let Some(old_task) = old_task {
            old_task.cancel();
        }

        chunk_map.notify_generation_refill();
    }

    /// Gets access to the chunk if it has reached the given status.
    #[inline]
    pub fn try_chunk(&self, status: ChunkStatus) -> Option<RwLockReadGuard<'_, ChunkAccess>> {
        let ready = {
            let chunk_result = self.chunk_result.borrow();
            matches!(&*chunk_result, ChunkResult::Ok(s) if status <= *s)
        };

        if ready { Some(self.data.read()) } else { None }
    }

    /// Waits until the chunk has reached the given status.
    pub fn await_chunk(
        &self,
        status: ChunkStatus,
    ) -> impl Future<Output = Option<RwLockReadGuard<'_, ChunkAccess>>> {
        let mut subscriber = self.sender.subscribe();
        async move {
            loop {
                let ready = {
                    let chunk_result = subscriber.borrow_and_update();
                    matches!(&*chunk_result, ChunkResult::Ok(s) if status <= *s)
                };

                if ready {
                    return Some(self.data.read());
                }

                if self.is_status_disallowed(status) {
                    return None;
                }

                if subscriber.changed().await.is_err() {
                    log::error!("Failed to wait for chunk access");
                    return None;
                }
            }
        }
    }

    /// Waits until the chunk has reached the given status without reading chunk data.
    pub fn await_chunk_status(
        &self,
        status: ChunkStatus,
    ) -> impl Future<Output = Option<ChunkStatus>> + '_ {
        let mut subscriber = self.sender.subscribe();
        async move {
            loop {
                let ready = {
                    let chunk_result = subscriber.borrow_and_update();
                    match &*chunk_result {
                        ChunkResult::Ok(current_status) if status <= *current_status => {
                            Some(*current_status)
                        }
                        ChunkResult::Ok(_) | ChunkResult::Unloaded => None,
                    }
                };

                if ready.is_some() {
                    return ready;
                }

                if self.is_status_disallowed(status) {
                    return None;
                }

                if subscriber.changed().await.is_err() {
                    log::error!("Failed to wait for chunk status");
                    return None;
                }
            }
        }
    }

    /// Gets the persisted status of the chunk.
    pub fn persisted_status(&self) -> Option<ChunkStatus> {
        let chunk_result = self.chunk_result.borrow();
        match &*chunk_result {
            ChunkResult::Ok(s) => Some(*s),
            ChunkResult::Unloaded => None,
        }
    }

    /// Applies a step to the chunk.
    ///
    /// The `cancel_token` is from the owning generation task — dependency wait
    /// futures are raced against it so they bail out when the task is cancelled.
    ///
    /// # Panics
    /// Panics if the target status is not Empty and has no parent, or if the
    /// chunk status is invalid during generation.
    pub fn apply_step(
        self: &Arc<Self>,
        step: &'static ChunkStep,
        chunk_map: &Arc<ChunkMap>,
        cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        thread_pool: Arc<rayon::ThreadPool>,
        cancel_token: CancellationToken,
    ) -> Option<NeighborReady> {
        let target_status = step.target_status;

        if self.is_status_disallowed(target_status) {
            return None;
        }

        if !self.acquire_status_bump(target_status) {
            let self_clone = self.clone();
            return Some(Box::pin(async move {
                tokio::select! {
                    () = cancel_token.cancelled() => None,
                    result = self_clone.await_chunk_status(target_status) => result.map(|_| ()),
                }
            }));
        }

        let cache = cache.clone();
        let context = chunk_map.world_gen_context.clone();
        let self_clone = self.clone();
        let storage = chunk_map.storage.clone();

        let future = chunk_map.task_tracker.spawn(async move {
            let result = if target_status == ChunkStatus::Empty {
                Self::apply_empty_step(self_clone, step, context, cache, storage, thread_pool).await
            } else {
                Self::apply_generated_step(self_clone, step, context, cache, thread_pool).await
            };

            #[cfg(feature = "slow_chunk_gen")]
            if result.is_some() && SLOW_CHUNK_GEN.load(Ordering::Relaxed) {
                sleep(Duration::from_millis(200)).await;
            }

            result
        });

        Some(Box::pin(async move {
            match future.await {
                Ok(result) => result,
                Err(e) => {
                    log::error!("Chunk generation task panicked: {e}");
                    None
                }
            }
        }))
    }

    async fn apply_empty_step(
        holder: Arc<Self>,
        step: &'static ChunkStep,
        context: Arc<WorldGenContext>,
        cache: Arc<StaticCache2D<Arc<ChunkHolder>>>,
        storage: Arc<ChunkStorage>,
        thread_pool: Arc<rayon::ThreadPool>,
    ) -> Option<()> {
        let target_status = step.target_status;
        let chunk_exists = match storage.acquire_chunk(holder.pos).await {
            Ok(chunk_exists) => chunk_exists,
            Err(error) => {
                tracing::error!(
                    chunk = ?holder.pos,
                    "Failed to acquire chunk storage before load/generation: {error}",
                );
                return None;
            }
        };

        if holder.is_status_disallowed(target_status) {
            tracing::debug!(
                chunk = ?holder.pos,
                ?target_status,
                load_level = ?holder.load_level(),
                simulation_level = ?holder.simulation_level(),
                current_status = ?holder.persisted_status(),
                "Dropping storage load after chunk holder target became disallowed before load/generation: chunk={:?}, target_status={:?}, load_level={:?}, simulation_level={:?}, current_status={:?}",
                holder.pos,
                target_status,
                holder.load_level(),
                holder.simulation_level(),
                holder.persisted_status(),
            );
            if let Err(error) = storage.release_chunk(holder.pos).await {
                tracing::error!(
                    chunk = ?holder.pos,
                    "Failed to release canceled chunk storage task: {error}",
                );
            }
            return None;
        }

        if chunk_exists {
            return Self::apply_existing_empty_step(&holder, target_status, &context, &storage)
                .await;
        }

        if holder.is_status_disallowed(target_status) {
            tracing::debug!(
                chunk = ?holder.pos,
                ?target_status,
                load_level = ?holder.load_level(),
                simulation_level = ?holder.simulation_level(),
                current_status = ?holder.persisted_status(),
                "Dropping storage load after chunk holder target became disallowed after load attempt: chunk={:?}, target_status={:?}, load_level={:?}, simulation_level={:?}, current_status={:?}",
                holder.pos,
                target_status,
                holder.load_level(),
                holder.simulation_level(),
                holder.persisted_status(),
            );
            if let Err(error) = storage.release_chunk(holder.pos).await {
                tracing::error!(
                    chunk = ?holder.pos,
                    "Failed to release canceled chunk storage task: {error}",
                );
            }
            return None;
        }

        let holder_for_notify = holder.clone();
        let world = context.world();
        Self::run_step_task(thread_pool, step, context, cache, holder).await;
        holder_for_notify.finish_generation_status(target_status);
        if target_status == ChunkStatus::Empty {
            world.on_entity_chunk_loaded(holder_for_notify.pos);
        }
        Some(())
    }

    async fn apply_existing_empty_step(
        holder: &Arc<Self>,
        target_status: ChunkStatus,
        context: &Arc<WorldGenContext>,
        storage: &Arc<ChunkStorage>,
    ) -> Option<()> {
        let loaded = match storage
            .load_chunk(
                holder.pos,
                holder.min_y(),
                holder.height(),
                context.weak_world(),
            )
            .await
        {
            Ok(Some(loaded)) => loaded,
            Ok(None) => {
                tracing::error!(
                    chunk = ?holder.pos,
                    "Chunk storage reported an existing chunk but load returned no chunk; aborting generation to avoid overwriting saved data",
                );
                if let Err(error) = storage.release_chunk(holder.pos).await {
                    tracing::error!(
                        chunk = ?holder.pos,
                        "Failed to release chunk storage after missing load result: {error}",
                    );
                }
                return None;
            }
            Err(error) => {
                tracing::error!(
                    chunk = ?holder.pos,
                    "Failed to load existing chunk; aborting generation to avoid overwriting saved data: {error}",
                );
                if let Err(release_error) = storage.release_chunk(holder.pos).await {
                    tracing::error!(
                        chunk = ?holder.pos,
                        "Failed to release chunk storage after load failure: {release_error}",
                    );
                }
                return None;
            }
        };

        let loaded_status = loaded.status;
        if holder.is_status_disallowed(target_status) {
            tracing::debug!(
                chunk = ?holder.pos,
                ?target_status,
                ?loaded_status,
                load_level = ?holder.load_level(),
                simulation_level = ?holder.simulation_level(),
                current_status = ?holder.persisted_status(),
                "Dropping storage load that completed after chunk holder target became disallowed: chunk={:?}, target_status={:?}, loaded_status={:?}, load_level={:?}, simulation_level={:?}, current_status={:?}",
                holder.pos,
                target_status,
                loaded_status,
                holder.load_level(),
                holder.simulation_level(),
                holder.persisted_status(),
            );
            if let Err(error) = storage.release_chunk(holder.pos).await {
                tracing::error!(
                    chunk = ?holder.pos,
                    "Failed to release canceled chunk storage load: {error}",
                );
            }
            return None;
        }

        holder.insert_chunk(loaded.chunk, loaded_status);
        context.world().on_entity_chunk_loaded(holder.pos);
        if !loaded.pending_entities.is_empty() {
            context.world().register_loaded_chunk_entities(
                holder.pos,
                loaded_status,
                loaded.pending_entities,
            );
        }
        Some(())
    }

    async fn apply_generated_step(
        holder: Arc<Self>,
        step: &'static ChunkStep,
        context: Arc<WorldGenContext>,
        cache: Arc<StaticCache2D<Arc<ChunkHolder>>>,
        thread_pool: Arc<rayon::ThreadPool>,
    ) -> Option<()> {
        let target_status = step.target_status;
        let Some(parent_status) = target_status.parent() else {
            panic!("Target status must have parent if not Empty");
        };
        let has_parent = holder
            .persisted_status()
            .is_some_and(|status| parent_status <= status);
        let holder_for_notify = holder.clone();

        assert!(has_parent, "Parent chunk missing");

        Self::run_step_task(thread_pool, step, context, cache, holder).await;
        holder_for_notify.finish_generation_status(target_status);
        Some(())
    }

    async fn run_step_task(
        thread_pool: Arc<rayon::ThreadPool>,
        step: &'static ChunkStep,
        context: Arc<WorldGenContext>,
        cache: Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<Self>,
    ) {
        let task = step.task;
        rayon_spawn(&thread_pool, move || {
            task(context, step, &cache, holder);
        })
        .await;
    }

    fn acquire_status_bump(&self, status: ChunkStatus) -> bool {
        let status_index = status.get_index();
        let parent_index = status
            .parent()
            .map_or(usize::MAX, super::chunk_access::ChunkStatus::get_index);

        let previous_started = self.started_work.compare_exchange(
            parent_index,
            status_index,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );

        match previous_started {
            Ok(_) => true,
            Err(current) => {
                if current != usize::MAX && current >= status_index {
                    false
                } else {
                    panic!(
                        "Unexpected started work status: {current:?} (index {current}) while trying to start: {status:?} (index {status_index})"
                    );
                }
            }
        }
    }

    /// Upgrades the chunk to a full chunk.
    ///
    /// If the chunk is already a `LevelChunk` (e.g., loaded from disk), this is a no-op.
    ///
    /// # Arguments
    /// * `level` - Weak reference to the world for the `LevelChunk`
    ///
    /// # Panics
    /// Panics if the chunk is not at `ProtoChunk` stage or already full.
    pub fn upgrade_to_full(&self, level: Weak<World>) {
        let world = level.upgrade();
        let promoted_entities = self.data.with_write(|chunk| {
            use std::mem::replace;
            let owned = replace(chunk, ChunkAccess::Unloaded);

            match owned {
                ChunkAccess::Proto(proto) => {
                    let min_y = proto.min_y();
                    let height = proto.height();
                    let LevelChunkPromotion {
                        chunk: full,
                        pending_entities,
                    } = LevelChunk::from_proto(proto, min_y, height, level);
                    let pos = full.pos;
                    *chunk = ChunkAccess::Full(full);
                    Some((pos, pending_entities))
                }
                ChunkAccess::Full(full) => {
                    *chunk = ChunkAccess::Full(full);
                    None
                }
                ChunkAccess::Unloaded => panic!("Chunk is unloaded, cannot upgrade to full"),
            }
        });
        if let (Some(world), Some((pos, pending_entities))) = (world, promoted_entities) {
            world.register_loaded_chunk_entities(pos, ChunkStatus::Full, pending_entities);
        }
    }

    fn post_process_generation(&self) {
        let postprocessing = {
            let chunk = self.data.read();
            let ChunkAccess::Full(full) = &*chunk else {
                return;
            };
            full.get_level().and_then(|world| {
                full.take_postprocessing()
                    .map(|postprocessing| (world, full.pos, full.min_y(), postprocessing))
            })
        };

        if let Some((world, pos, min_y, postprocessing)) = postprocessing {
            LevelChunk::post_process_generation(&world, pos, min_y, postprocessing);
        }
    }

    /// Finishes a generated status on the async scheduler after the Rayon task returns.
    fn finish_generation_status(&self, status: ChunkStatus) {
        {
            let stored_chunk = self.data.read();
            if let ChunkAccess::Proto(proto_chunk) = &*stored_chunk
                && proto_chunk.status() < status
            {
                proto_chunk.set_status(status);
                stored_chunk.mark_dirty();
            }
        }

        self.sender.send_modify(|chunk| match chunk {
            ChunkResult::Ok(current_status) if *current_status < status => {
                *current_status = status;
            }
            ChunkResult::Unloaded => {
                *chunk = ChunkResult::Ok(status);
            }
            ChunkResult::Ok(_) => {}
        });

        self.post_publish_status_hooks(status);
    }

    fn post_publish_status_hooks(&self, status: ChunkStatus) {
        if status == ChunkStatus::Full {
            self.post_process_generation();
        }
    }

    /// Inserts a chunk into the holder with a specific status.
    /// This notifies watchers - use `insert_chunk_no_notify` + separate notification
    /// if calling from a rayon thread to avoid contention.
    pub fn insert_chunk(&self, chunk: ChunkAccess, status: ChunkStatus) {
        self.data.with_write(|c| *c = chunk);
        self.sender.send_replace(ChunkResult::Ok(status));
    }

    /// Inserts a chunk into the holder without notifying watchers.
    /// The caller is responsible for notifying via the completion channel.
    pub(crate) fn insert_chunk_no_notify(&self, chunk: ChunkAccess) {
        self.data.with_write(|c| *c = chunk);
    }

    /// Wakes all `await_chunk` watchers without changing the chunk result.
    /// This allows futures stuck in `subscriber.changed().await` to re-check
    /// `is_status_disallowed` and bail out during chunk unload.
    pub fn wake_all_watchers(&self) {
        self.sender.send_modify(|_| {});
    }

    /// Cancels the current generation task.
    pub fn cancel_generation_task(&self) {
        let mut task_guard = self.generation_task.lock();
        self.generation_task_target
            .store(STATUS_NONE, Ordering::Release);
        if let Some(task) = task_guard.take() {
            task.cancel();
        }
    }

    /// Clears the current generation task if it is still the supplied task.
    pub(crate) fn clear_generation_task_if_current(&self, task: &Arc<ChunkGenerationTask>) {
        let mut task_guard = self.generation_task.lock();
        if task_guard
            .as_ref()
            .is_some_and(|current_task| Arc::ptr_eq(current_task, task))
        {
            task_guard.take();
            self.generation_task_target
                .store(STATUS_NONE, Ordering::Release);
        }
    }
}

fn rayon_spawn<F, R>(thread_pool: &rayon::ThreadPool, func: F) -> impl Future<Output = R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static + Debug,
{
    let (sender, receiver) = oneshot::channel();
    thread_pool.spawn(move || {
        sender.send(func()).expect("Failed to send result");
    });
    async move { receiver.await.expect("Failed to receive rayon task result") }
}
