//! `ChunkHolder` manages chunk state and asynchronous generation tasks.
use futures::Future;
use parking_lot::RwLockReadGuard;
use rustc_hash::FxHashSet;
use std::fmt::Debug;
use std::mem;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use steel_utils::locks::SyncRwLock;
use steel_utils::{BlockPos, ChunkPos, PackedSectionBlockPos, SectionPos, locks::SyncMutex};
use tokio::sync::{oneshot, watch};
#[cfg(feature = "slow_chunk_gen")]
use tokio::time::sleep;

#[cfg(feature = "slow_chunk_gen")]
use std::time::Duration;

/// When `true`, each chunk generation stage sleeps 200 ms after completing.
/// Set by the spawn progress display to make the terminal grid visible.
#[cfg(feature = "slow_chunk_gen")]
pub static SLOW_CHUNK_GEN: AtomicBool = AtomicBool::new(false);

use crate::chunk::chunk_generation_task::{NeighborReady, StaticCache2D};
use crate::chunk::chunk_ticket_manager::{ChunkTicketLevel, generation_status, is_full, is_ticked};
use crate::chunk::light::{
    LightLayer, LightSectionRange, LightWorkWindowGate, LightWorkWindowReservation,
};
use crate::chunk_saver::ChunkStorage;
use crate::entity::EntityVisibility;
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

#[derive(Debug, Default)]
struct ChangedLightSectionSets {
    sky: FxHashSet<SectionPos>,
    block: FxHashSet<SectionPos>,
}

/// Pending light sections to send to players tracking a chunk.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ChangedLightSections {
    /// Changed sky-light sections.
    pub sky: Vec<SectionPos>,
    /// Changed block-light sections.
    pub block: Vec<SectionPos>,
}

impl ChangedLightSections {
    /// Returns true when no light sections changed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.sky.is_empty() && self.block.is_empty()
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
    /// Whether this holder is already queued for the next broadcast flush.
    queued_for_broadcast: AtomicBool,
    /// Monotonic revision for client-visible chunk packet content.
    packet_content_revision: AtomicU64,
    /// Per-section sets of changed block positions.
    /// Index is `(block_y - min_y) / 16`.
    changed_blocks_per_section: Box<[SyncMutex<FxHashSet<PackedSectionBlockPos>>]>,
    /// Changed light sections grouped by light layer.
    changed_light_sections: SyncMutex<ChangedLightSectionSets>,
}

struct StatusWorkClaim {
    holder: Arc<ChunkHolder>,
    status: ChunkStatus,
}

impl StatusWorkClaim {
    const fn new(holder: Arc<ChunkHolder>, status: ChunkStatus) -> Self {
        Self { holder, status }
    }
}

impl Drop for StatusWorkClaim {
    fn drop(&mut self) {
        self.holder.release_status_work_claim(self.status);
    }
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
            queued_for_broadcast: AtomicBool::new(false),
            packet_content_revision: AtomicU64::new(0),
            changed_blocks_per_section,
            changed_light_sections: SyncMutex::new(ChangedLightSectionSets::default()),
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

    pub(crate) fn entity_visibility(&self) -> EntityVisibility {
        if self.try_chunk(ChunkStatus::Full).is_none() {
            return EntityVisibility::Hidden;
        }

        if !self.load_level().is_some_and(is_full) {
            return EntityVisibility::Hidden;
        }

        if is_ticked(self.simulation_level()) {
            EntityVisibility::Ticking
        } else {
            EntityVisibility::Tracked
        }
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
        if pos.0.y < self.min_y || pos.0.y >= self.min_y + self.height {
            return false;
        }

        let section_index = ((pos.0.y - self.min_y) / 16) as usize;
        if section_index >= self.changed_blocks_per_section.len() {
            return false;
        }

        let packed = SectionPos::section_relative_pos(pos);
        self.changed_blocks_per_section[section_index]
            .lock()
            .insert(packed);
        self.mark_packet_content_changed();
        self.has_changed_sections.store(true, Ordering::Release);

        !self.queued_for_broadcast.swap(true, Ordering::AcqRel)
    }

    /// Records a light-section change for a full chunk and marks saved light data dirty.
    ///
    /// Returns `true` if this is the first pending broadcast change for the chunk holder.
    pub fn light_changed(&self, layer: LightLayer, section_pos: SectionPos) -> bool {
        let Some(ready_for_packet) = self.mark_valid_light_section_dirty(section_pos) else {
            return false;
        };
        if !ready_for_packet {
            return false;
        }
        self.mark_packet_content_changed();

        let inserted = {
            let mut guard = self.changed_light_sections.lock();
            match layer {
                LightLayer::Sky => guard.sky.insert(section_pos),
                LightLayer::Block => guard.block.insert(section_pos),
            }
        };

        if !inserted {
            return false;
        }

        !self.queued_for_broadcast.swap(true, Ordering::AcqRel)
    }

    /// Marks saved light data dirty without queuing client-visible changes.
    pub fn mark_light_section_dirty(&self, section_pos: SectionPos) -> bool {
        self.mark_valid_light_section_dirty(section_pos).is_some()
    }

    fn mark_valid_light_section_dirty(&self, section_pos: SectionPos) -> Option<bool> {
        if section_pos.x() != self.pos.0.x || section_pos.z() != self.pos.0.y {
            return None;
        }

        let Ok(range) = LightSectionRange::from_world_height(self.min_y, self.height) else {
            return None;
        };
        range.section_index(section_pos.y())?;

        let chunk = self.data.read();
        match &*chunk {
            ChunkAccess::Full(_) => {
                chunk.mark_dirty();
                Some(true)
            }
            ChunkAccess::Proto(_) => {
                chunk.mark_dirty();
                Some(false)
            }
            ChunkAccess::Unloaded => None,
        }
    }

    /// Returns whether there are pending changes to broadcast.
    pub fn has_changes_to_broadcast(&self) -> bool {
        self.queued_for_broadcast.load(Ordering::Acquire)
    }

    /// Allows later changes to enqueue this holder for a future broadcast.
    pub fn clear_broadcast_queued(&self) {
        self.queued_for_broadcast.store(false, Ordering::Release);
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

    /// Takes all pending light-section changes.
    pub fn take_changed_light_sections(&self) -> ChangedLightSections {
        let mut guard = self.changed_light_sections.lock();
        ChangedLightSections {
            sky: guard.sky.drain().collect(),
            block: guard.block.drain().collect(),
        }
    }

    /// Marks the holder's client-visible chunk packet content as changed.
    pub fn mark_packet_content_changed(&self) {
        self.packet_content_revision.fetch_add(1, Ordering::AcqRel);
    }

    /// Returns the current client-visible content revision.
    pub fn packet_content_revision(&self) -> u64 {
        self.packet_content_revision.load(Ordering::Acquire)
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

    fn await_claimed_chunk_status(
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

                if self.is_status_disallowed(status) || !self.status_work_covers(status) {
                    return None;
                }

                if subscriber.changed().await.is_err() {
                    log::error!("Failed to wait for claimed chunk status");
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
    /// Cancellation is handled structurally by the owning generation task: its
    /// `run` loop races the whole `join_all` of dependency-wait futures against
    /// its cancel token and drops them on cancellation, so the returned futures
    /// don't each re-check it. A failed dependency surfaces as
    /// `await_chunk_status` returning `None`.
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
    ) -> Option<NeighborReady> {
        let target_status = step.target_status;

        if self.is_status_disallowed(target_status) {
            return None;
        }

        if target_status == ChunkStatus::Light {
            let light_work_window_gate = chunk_map.light_work_window_gate();
            let Some(light_work_window_reservation) =
                light_work_window_gate.try_reserve_centered(self.pos)
            else {
                return Some(Self::await_light_work_window_and_apply_step(
                    Arc::clone(self),
                    step,
                    Arc::clone(chunk_map),
                    Arc::clone(cache),
                    thread_pool,
                    light_work_window_gate,
                ));
            };

            return self.apply_step_with_light_work_window_reservation(
                step,
                chunk_map,
                cache,
                thread_pool,
                Some(light_work_window_reservation),
            );
        }

        self.apply_step_with_light_work_window_reservation(
            step,
            chunk_map,
            cache,
            thread_pool,
            None,
        )
    }

    fn await_light_work_window_and_apply_step(
        holder: Arc<Self>,
        step: &'static ChunkStep,
        chunk_map: Arc<ChunkMap>,
        cache: Arc<StaticCache2D<Arc<ChunkHolder>>>,
        thread_pool: Arc<rayon::ThreadPool>,
        light_work_window_gate: Arc<LightWorkWindowGate>,
    ) -> NeighborReady {
        Box::pin(async move {
            let light_work_window_reservation =
                light_work_window_gate.reserve_centered(holder.pos).await;
            let ready = holder.apply_step_with_light_work_window_reservation(
                step,
                &chunk_map,
                &cache,
                thread_pool,
                Some(light_work_window_reservation),
            )?;
            ready.await
        })
    }

    fn apply_step_with_light_work_window_reservation(
        self: &Arc<Self>,
        step: &'static ChunkStep,
        chunk_map: &Arc<ChunkMap>,
        cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        thread_pool: Arc<rayon::ThreadPool>,
        light_work_window_reservation: Option<LightWorkWindowReservation>,
    ) -> Option<NeighborReady> {
        let target_status = step.target_status;
        debug_assert!(
            target_status != ChunkStatus::Light || light_work_window_reservation.is_some()
        );

        if self.is_status_disallowed(target_status) {
            return None;
        }

        let Some(status_claim) = self.claim_status_work(target_status) else {
            // Another task is already generating this chunk to `target_status`;
            // just wait for it. Parent cancellation is handled by the owning
            // task's run loop dropping this future; a failed dependency returns
            // `None` from `await_claimed_chunk_status`.
            let self_clone = self.clone();
            return Some(Box::pin(async move {
                self_clone
                    .await_claimed_chunk_status(target_status)
                    .await
                    .map(|_| ())
            }));
        };

        let cache = cache.clone();
        let context = chunk_map.world_gen_context.clone();
        let self_clone = self.clone();
        let storage = chunk_map.storage.clone();

        let future = chunk_map.task_tracker.spawn(async move {
            // Keep the claim alive for the producer task so Drop can roll back abandoned work.
            let _status_claim = status_claim;
            let result = if target_status == ChunkStatus::Empty {
                Self::apply_empty_step(self_clone, step, context, cache, storage, thread_pool).await
            } else {
                Self::apply_generated_step(
                    self_clone,
                    step,
                    context,
                    cache,
                    thread_pool,
                    light_work_window_reservation,
                )
                .await
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
        context
            .world()
            .update_entity_chunk_visibility(holder.pos, holder.entity_visibility());
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
        light_work_window_reservation: Option<LightWorkWindowReservation>,
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
        drop(light_work_window_reservation);
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

    fn claim_status_work(self: &Arc<Self>, status: ChunkStatus) -> Option<StatusWorkClaim> {
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
            Ok(_) => Some(StatusWorkClaim::new(Arc::clone(self), status)),
            Err(current) => {
                if current != usize::MAX && current >= status_index {
                    None
                } else {
                    panic!(
                        "Unexpected started work status: {current:?} (index {current}) while trying to start: {status:?} (index {status_index})"
                    );
                }
            }
        }
    }

    fn release_status_work_claim(&self, status: ChunkStatus) {
        let status_index = status.get_index();
        let rollback_index = self
            .persisted_status()
            .map_or(usize::MAX, super::chunk_access::ChunkStatus::get_index);

        if rollback_index != usize::MAX && rollback_index >= status_index {
            return;
        }

        if self
            .started_work
            .compare_exchange(
                status_index,
                rollback_index,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
        {
            self.wake_all_watchers();
        }
    }

    fn mark_status_work_published(&self, status: ChunkStatus) {
        let status_index = status.get_index();
        let mut current = self.started_work.load(Ordering::Acquire);

        loop {
            if current != usize::MAX && current >= status_index {
                return;
            }

            match self.started_work.compare_exchange(
                current,
                status_index,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return,
                Err(next) => current = next,
            }
        }
    }

    fn status_work_covers(&self, status: ChunkStatus) -> bool {
        let current = self.started_work.load(Ordering::Acquire);
        current != usize::MAX && current >= status.get_index()
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
            world.update_entity_chunk_visibility(pos, self.entity_visibility());
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

        self.mark_status_work_published(status);
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
        if let ChunkAccess::Proto(proto) = &chunk {
            debug_assert!(
                status < ChunkStatus::Full,
                "full status must be stored as a LevelChunk"
            );
            proto.set_status(status);
        }
        self.data.with_write(|c| *c = chunk);
        self.mark_status_work_published(status);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::init_behaviors;
    use crate::chunk::proto_chunk::ProtoChunk;
    use crate::chunk::section::{ChunkSection, Sections};
    use steel_registry::test_support::init_test_registry;

    fn init_chunk_test_registry() {
        init_test_registry();
        init_behaviors();
    }

    fn test_holder() -> Arc<ChunkHolder> {
        Arc::new(ChunkHolder::new(
            ChunkPos::new(0, 0),
            ChunkTicketLevel::FULL_CHUNK,
            Some(ChunkTicketLevel::FULL_CHUNK),
            0,
            16,
        ))
    }

    fn test_proto_chunk(status: ChunkStatus) -> ProtoChunk {
        let proto = ProtoChunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        proto.set_status(status);
        proto
    }

    #[test]
    fn insert_chunk_synchronizes_proto_status_with_published_status() {
        init_chunk_test_registry();
        let holder = test_holder();
        let proto = ProtoChunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );

        holder.insert_chunk(ChunkAccess::Proto(proto), ChunkStatus::Light);

        let Some(chunk) = holder.try_chunk(ChunkStatus::Light) else {
            panic!("inserted chunk should be available at published status");
        };
        let ChunkAccess::Proto(proto) = &*chunk else {
            panic!("inserted test chunk should remain proto");
        };
        assert_eq!(proto.status(), ChunkStatus::Light);
    }

    #[test]
    fn unpublished_status_claim_rolls_back_to_unloaded() {
        let holder = test_holder();
        let claim = holder
            .claim_status_work(ChunkStatus::Empty)
            .expect("empty status should be claimable");

        assert!(holder.claim_status_work(ChunkStatus::Empty).is_none());

        drop(claim);

        assert!(!holder.status_work_covers(ChunkStatus::Empty));
        let retry = holder
            .claim_status_work(ChunkStatus::Empty)
            .expect("abandoned empty status should be claimable again");
        drop(retry);
    }

    #[test]
    fn unpublished_child_claim_rolls_back_to_published_parent() {
        init_chunk_test_registry();
        let holder = test_holder();
        holder.insert_chunk(
            ChunkAccess::Proto(test_proto_chunk(ChunkStatus::Empty)),
            ChunkStatus::Empty,
        );

        let claim = holder
            .claim_status_work(ChunkStatus::StructureStarts)
            .expect("child status should be claimable after parent is published");

        drop(claim);

        assert!(holder.status_work_covers(ChunkStatus::Empty));
        assert!(!holder.status_work_covers(ChunkStatus::StructureStarts));
        let retry = holder
            .claim_status_work(ChunkStatus::StructureStarts)
            .expect("abandoned child status should be claimable again");
        drop(retry);
    }

    #[test]
    fn empty_claim_can_publish_a_higher_loaded_status() {
        init_chunk_test_registry();
        let holder = test_holder();
        let empty_claim = holder
            .claim_status_work(ChunkStatus::Empty)
            .expect("empty status should be claimable");

        holder.insert_chunk(
            ChunkAccess::Proto(test_proto_chunk(ChunkStatus::StructureStarts)),
            ChunkStatus::StructureStarts,
        );
        drop(empty_claim);

        assert!(holder.status_work_covers(ChunkStatus::StructureStarts));
        assert!(!holder.status_work_covers(ChunkStatus::StructureReferences));
        let next_claim = holder
            .claim_status_work(ChunkStatus::StructureReferences)
            .expect("next status should be claimable from loaded status");
        drop(next_claim);
    }

    #[tokio::test]
    async fn claimed_status_waiter_finishes_when_claim_is_abandoned() {
        let holder = test_holder();
        let claim = holder
            .claim_status_work(ChunkStatus::Empty)
            .expect("empty status should be claimable");
        let waiter = holder.await_claimed_chunk_status(ChunkStatus::Empty);

        drop(claim);

        assert!(waiter.await.is_none());
    }
}
