//! `ChunkHolder` manages chunk state and asynchronous generation tasks.
use futures::Future;
use rustc_hash::FxHashSet;
use std::fmt::Debug;
use std::mem;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use steel_utils::{BlockPos, ChunkPos, PackedSectionBlockPos, SectionPos, locks::SyncMutex};
use tokio::sync::{Notify, oneshot};
#[cfg(feature = "slow_chunk_gen")]
use tokio::time::sleep;

#[cfg(feature = "slow_chunk_gen")]
use std::time::Duration;

/// When `true`, each chunk generation stage sleeps 200 ms after completing.
/// Set by the spawn progress display to make the terminal grid visible.
#[cfg(feature = "slow_chunk_gen")]
pub static SLOW_CHUNK_GEN: AtomicBool = AtomicBool::new(false);

use crate::chunk::chunk_generation_task::{NeighborReady, StaticCache2D};
use crate::chunk::chunk_ticket_manager::{
    ChunkTicketLevel, generation_status, is_entity_ticking, is_full,
};
use crate::chunk::full_chunk_readiness::FullPublicationQueue;
use crate::chunk::light::{
    LightLayer, LightSectionRange, LightWorkWindowGate, LightWorkWindowReservation,
};
use crate::chunk_saver::ChunkStorage;
use crate::entity::EntityVisibility;
use crate::worldgen::WorldGenContext;
use crate::{
    ChunkMap,
    chunk::{
        Chunk,
        chunk_generation_task::ChunkGenerationTask,
        chunk_pyramid::ChunkStep,
        full_chunk::{FullChunkPromotion, FullChunkRef},
        status::ChunkStatus,
    },
};

const STATUS_NONE: u8 = u8::MAX;
const UNPUBLISHED_STATUS: u8 = 0;
const NO_TICKET_LEVEL: u8 = u8::MAX;
const SAVE_LIFECYCLE_ACTIVE: u8 = 0;
const SAVE_LIFECYCLE_UNLOADING: u8 = 1;
const SAVE_LIFECYCLE_PREPARING: u8 = 2;

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

const fn encoded_published_status(status: ChunkStatus) -> u8 {
    status.get_index() as u8 + 1
}

fn decoded_published_status(status: u8) -> Option<ChunkStatus> {
    if status == UNPUBLISHED_STATUS {
        return None;
    }

    let decoded = ChunkStatus::from_index(usize::from(status - 1));
    assert!(
        decoded.is_some(),
        "invalid published chunk status: {status}"
    );
    decoded
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub(crate) enum TickingReadiness {
    Unready,
    BlockTicking,
    EntityTicking,
}

/// Exact ticking-readiness generation captured by concurrent consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TickingReadinessSnapshot(u64);

impl TickingReadinessSnapshot {
    #[must_use]
    pub(crate) const fn readiness(self) -> TickingReadiness {
        match self.0 & 0b11 {
            0 => TickingReadiness::Unready,
            1 => TickingReadiness::BlockTicking,
            2 => TickingReadiness::EntityTicking,
            _ => unreachable!(),
        }
    }

    #[must_use]
    pub(crate) const fn is_block_ticking(self) -> bool {
        matches!(
            self.readiness(),
            TickingReadiness::BlockTicking | TickingReadiness::EntityTicking
        )
    }

    #[must_use]
    pub(crate) const fn is_entity_ticking(self) -> bool {
        matches!(self.readiness(), TickingReadiness::EntityTicking)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PostProcessGenerationError {
    ChunkNotFull,
    WorldUnavailable,
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

/// Holds chunk data and coordinates asynchronous generation work.
///
/// `published_status` is released only after the corresponding data and Full
/// tick containers are installed. Synchronous readers acquire it before
/// reading `data`; `status_changed` wakes async waiters so they can re-check
/// the atomic state.
pub struct ChunkHolder {
    data: OnceLock<Chunk>,
    published_status: AtomicU8,
    status_changed: Notify,
    generation_task: SyncMutex<Option<Arc<ChunkGenerationTask>>>,
    generation_task_target: AtomicU8,
    pos: ChunkPos,
    /// The current loading ticket level of the chunk.
    load_level: AtomicU8,
    /// The current simulation ticket level of the chunk.
    simulation_level: AtomicU8,
    /// The highest status that has started work.
    started_work: AtomicUsize,
    /// Number of save dependencies that have not completed yet.
    active_save_dependencies: AtomicUsize,
    /// Coordinates unloading revival with the short immutable save-preparation phase.
    save_lifecycle: AtomicU8,
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
    /// Packed ticking readiness generation. The low two bits store `TickingReadiness`.
    ticking_readiness: AtomicU64,
    /// Whether Full post-load initialization completed and was published for readiness.
    full_status_initialized: AtomicBool,
    /// Weak sink for Full status publication notifications.
    full_publications: Weak<FullPublicationQueue>,
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

pub(crate) struct ChunkSaveDependency {
    holder: Arc<ChunkHolder>,
}

impl Drop for ChunkSaveDependency {
    fn drop(&mut self) {
        self.holder
            .active_save_dependencies
            .fetch_sub(1, Ordering::AcqRel);
    }
}

pub(crate) struct ChunkSavePreparationGuard {
    holder: Arc<ChunkHolder>,
}

impl Drop for ChunkSavePreparationGuard {
    fn drop(&mut self) {
        let result = self.holder.save_lifecycle.compare_exchange(
            SAVE_LIFECYCLE_PREPARING,
            SAVE_LIFECYCLE_UNLOADING,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        assert!(
            result.is_ok(),
            "chunk save preparation ended outside the preparing lifecycle"
        );
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
        Self::new_with_full_publications(
            pos,
            load_level,
            simulation_level,
            min_y,
            height,
            Weak::new(),
        )
    }

    pub(crate) fn new_with_full_publications(
        pos: ChunkPos,
        load_level: ChunkTicketLevel,
        simulation_level: Option<ChunkTicketLevel>,
        min_y: i32,
        height: i32,
        full_publications: Weak<FullPublicationQueue>,
    ) -> Self {
        let highest_allowed_status =
            generation_status(Some(load_level)).map_or(STATUS_NONE, |s| s.get_index() as u8);

        let section_count = (height / 16) as usize;
        let changed_blocks_per_section = (0..section_count)
            .map(|_| SyncMutex::new(FxHashSet::default()))
            .collect::<Box<[_]>>();

        Self {
            data: OnceLock::new(),
            published_status: AtomicU8::new(UNPUBLISHED_STATUS),
            status_changed: Notify::new(),
            generation_task: SyncMutex::new(None),
            generation_task_target: AtomicU8::new(STATUS_NONE),
            pos,
            load_level: AtomicU8::new(load_level.raw()),
            simulation_level: AtomicU8::new(optional_ticket_level_raw(simulation_level)),
            started_work: AtomicUsize::new(usize::MAX),
            active_save_dependencies: AtomicUsize::new(0),
            save_lifecycle: AtomicU8::new(SAVE_LIFECYCLE_ACTIVE),
            highest_allowed_status: AtomicU8::new(highest_allowed_status),
            min_y,
            height,
            has_changed_sections: AtomicBool::new(false),
            queued_for_broadcast: AtomicBool::new(false),
            packet_content_revision: AtomicU64::new(0),
            ticking_readiness: AtomicU64::new(0),
            full_status_initialized: AtomicBool::new(false),
            full_publications,
            changed_blocks_per_section,
            changed_light_sections: SyncMutex::new(ChangedLightSectionSets::default()),
        }
    }

    /// Returns the current load ticket level.
    pub fn load_level(&self) -> Option<ChunkTicketLevel> {
        optional_ticket_level_from_raw(self.load_level.load(Ordering::Relaxed))
    }

    /// Stores the current load ticket level and returns the previous level.
    pub(crate) fn swap_load_level(&self, level: ChunkTicketLevel) -> Option<ChunkTicketLevel> {
        optional_ticket_level_from_raw(self.load_level.swap(level.raw(), Ordering::Relaxed))
    }

    /// Clears the current load ticket level.
    pub(crate) fn clear_load_level(&self) {
        self.load_level.store(NO_TICKET_LEVEL, Ordering::Relaxed);
    }

    /// Returns the current simulation ticket level.
    pub fn simulation_level(&self) -> Option<ChunkTicketLevel> {
        optional_ticket_level_from_raw(self.simulation_level.load(Ordering::Relaxed))
    }

    /// Stores the current simulation ticket level.
    pub(crate) fn set_simulation_level(&self, level: Option<ChunkTicketLevel>) {
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

        if is_entity_ticking(self.simulation_level())
            && self.ticking_readiness_snapshot().is_entity_ticking()
        {
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
        if !self.ticking_readiness_snapshot().is_block_ticking()
            || pos.0.y < self.min_y
            || pos.0.y >= self.min_y + self.height
        {
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

        let status = self.published_status()?;
        let chunk = self.data.get()?;
        chunk.mark_dirty();
        Some(status == ChunkStatus::Full && self.ticking_readiness_snapshot().is_block_ticking())
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

    #[must_use]
    pub(crate) fn ticking_readiness_snapshot(&self) -> TickingReadinessSnapshot {
        TickingReadinessSnapshot(self.ticking_readiness.load(Ordering::Acquire))
    }

    #[must_use]
    pub(crate) fn is_full_status_initialized(&self) -> bool {
        self.full_status_initialized.load(Ordering::Acquire)
    }

    pub(crate) fn transition_ticking_readiness(
        &self,
        target: TickingReadiness,
    ) -> Option<TickingReadiness> {
        let mut current = self.ticking_readiness.load(Ordering::Acquire);
        loop {
            let snapshot = TickingReadinessSnapshot(current);
            let previous = snapshot.readiness();
            if previous == target {
                return None;
            }

            let generation = current >> 2;
            assert!(
                generation != u64::MAX >> 2,
                "chunk ticking readiness generation exhausted"
            );
            let next_generation = generation + 1;
            let next = (next_generation << 2) | target as u64;
            match self.ticking_readiness.compare_exchange(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(previous),
                Err(observed) => current = observed,
            }
        }
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
    pub fn try_chunk(&self, status: ChunkStatus) -> Option<&Chunk> {
        let published = self.published_status.load(Ordering::Acquire);
        (published >= encoded_published_status(status))
            .then(|| self.data.get())
            .flatten()
    }

    /// Gets the Full-only capability after Full status is published.
    #[must_use]
    pub fn try_full_chunk(&self) -> Option<FullChunkRef<'_>> {
        self.try_chunk(ChunkStatus::Full)
            .map(FullChunkRef::from_full_context)
    }

    /// Waits until the chunk has reached the given status.
    pub async fn await_chunk(&self, status: ChunkStatus) -> Option<&Chunk> {
        loop {
            // Register before checking the state so a concurrent publication
            // cannot land between the check and the wait.
            let notified = self.status_changed.notified();

            if self.published_status.load(Ordering::Acquire) >= encoded_published_status(status) {
                return self.data.get();
            }

            if self.is_status_disallowed(status) {
                return None;
            }

            notified.await;
        }
    }

    /// Waits until the chunk has reached the given status without reading chunk data.
    pub async fn await_chunk_status(&self, status: ChunkStatus) -> Option<ChunkStatus> {
        loop {
            let notified = self.status_changed.notified();
            let published = self.published_status();
            if published.is_some_and(|current| status <= current) {
                return published;
            }

            if self.is_status_disallowed(status) {
                return None;
            }

            notified.await;
        }
    }

    async fn await_claimed_chunk_status(&self, status: ChunkStatus) -> Option<ChunkStatus> {
        loop {
            let notified = self.status_changed.notified();
            let published = self.published_status();
            if published.is_some_and(|current| status <= current) {
                return published;
            }

            if self.is_status_disallowed(status) || !self.status_work_covers(status) {
                return None;
            }

            notified.await;
        }
    }

    /// Gets the published status of the chunk.
    pub fn published_status(&self) -> Option<ChunkStatus> {
        decoded_published_status(self.published_status.load(Ordering::Acquire))
    }

    /// Returns whether vanilla timed tickets may age for this chunk.
    #[must_use]
    pub fn is_ready_for_saving(&self) -> bool {
        self.active_save_dependencies.load(Ordering::Acquire) == 0
    }

    pub(crate) fn add_save_dependency(self: &Arc<Self>) -> ChunkSaveDependency {
        self.active_save_dependencies.fetch_add(1, Ordering::AcqRel);
        ChunkSaveDependency {
            holder: Arc::clone(self),
        }
    }

    /// Moves an active holder into the unloading lifecycle.
    pub(crate) fn begin_unloading(&self) {
        let result = self.save_lifecycle.compare_exchange(
            SAVE_LIFECYCLE_ACTIVE,
            SAVE_LIFECYCLE_UNLOADING,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        assert!(
            result.is_ok(),
            "an active chunk holder entered unloading from an invalid lifecycle"
        );
    }

    /// Reserves the unloading holder while its immutable save input is assembled.
    pub(crate) fn try_begin_save_preparation(
        self: &Arc<Self>,
    ) -> Option<ChunkSavePreparationGuard> {
        self.save_lifecycle
            .compare_exchange(
                SAVE_LIFECYCLE_UNLOADING,
                SAVE_LIFECYCLE_PREPARING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .ok()
            .map(|_| ChunkSavePreparationGuard {
                holder: Arc::clone(self),
            })
    }

    /// Attempts to reactivate an unloading holder without waiting for save preparation.
    pub(crate) fn try_revive_from_unloading(&self) -> bool {
        self.save_lifecycle
            .compare_exchange(
                SAVE_LIFECYCLE_UNLOADING,
                SAVE_LIFECYCLE_ACTIVE,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
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
        let save_dependency = self.add_save_dependency();

        let future = chunk_map.task_tracker.spawn(async move {
            // Keep the claim alive for the producer task so Drop can roll back abandoned work.
            let _status_claim = status_claim;
            let _save_dependency = save_dependency;
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
                current_status = ?holder.published_status(),
                "Dropping storage load after chunk holder target became disallowed before load/generation: chunk={:?}, target_status={:?}, load_level={:?}, simulation_level={:?}, current_status={:?}",
                holder.pos,
                target_status,
                holder.load_level(),
                holder.simulation_level(),
                holder.published_status(),
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
            match Self::apply_existing_empty_step(
                &holder,
                target_status,
                &context,
                &storage,
                &thread_pool,
            )
            .await
            {
                Some(true) => return Some(()),
                Some(false) => {}
                None => return None,
            }
        }

        if holder.is_status_disallowed(target_status) {
            tracing::debug!(
                chunk = ?holder.pos,
                ?target_status,
                load_level = ?holder.load_level(),
                simulation_level = ?holder.simulation_level(),
                current_status = ?holder.published_status(),
                "Dropping storage load after chunk holder target became disallowed after load attempt: chunk={:?}, target_status={:?}, load_level={:?}, simulation_level={:?}, current_status={:?}",
                holder.pos,
                target_status,
                holder.load_level(),
                holder.simulation_level(),
                holder.published_status(),
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
        thread_pool: &rayon::ThreadPool,
    ) -> Option<bool> {
        let loaded = match storage
            .load_chunk(
                holder.pos,
                holder.min_y(),
                holder.height(),
                context.weak_world(),
                thread_pool,
            )
            .await
        {
            Ok(Some(loaded)) => loaded,
            Ok(None) => {
                tracing::warn!(
                    chunk = ?holder.pos,
                    "Chunk storage entry disappeared or was discarded as corrupt; regenerating it",
                );
                return Some(false);
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
                current_status = ?holder.published_status(),
                "Dropping storage load that completed after chunk holder target became disallowed: chunk={:?}, target_status={:?}, loaded_status={:?}, load_level={:?}, simulation_level={:?}, current_status={:?}",
                holder.pos,
                target_status,
                loaded_status,
                holder.load_level(),
                holder.simulation_level(),
                holder.published_status(),
            );
            if let Err(error) = storage.release_chunk(holder.pos).await {
                tracing::error!(
                    chunk = ?holder.pos,
                    "Failed to release canceled chunk storage load: {error}",
                );
            }
            return None;
        }

        holder.store_and_publish_chunk_status(loaded.chunk, loaded_status);
        let world = context.world();
        world.on_entity_chunk_loaded(holder.pos);
        world.update_entity_chunk_visibility(holder.pos, holder.entity_visibility());
        if !loaded.pending_entities.is_empty() {
            world.register_loaded_chunk_entities(
                holder.pos,
                loaded_status,
                loaded.pending_entities,
            );
        }
        if loaded_status == ChunkStatus::Full {
            holder.publish_full();
        }
        Some(true)
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
            .published_status()
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
        let parent_index = status.parent().map_or(usize::MAX, ChunkStatus::get_index);

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
            .published_status()
            .map_or(usize::MAX, ChunkStatus::get_index);

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
    /// If the chunk is already Full (e.g., loaded from disk), this is a no-op.
    ///
    /// # Panics
    /// Panics if no chunk has been installed or Full runtime initialization repeats.
    pub(crate) fn upgrade_to_full(&self) {
        if self.published_status() == Some(ChunkStatus::Full) {
            return;
        }
        let Some(chunk) = self.data.get() else {
            panic!("cannot promote an uninitialized chunk holder");
        };
        let FullChunkPromotion {
            chunk: full,
            pending_entities,
        } = chunk.promote_to_full();
        let promoted_entities = Some((full.get_level(), chunk.pos, pending_entities));
        if let Some((world, pos, pending_entities)) = promoted_entities
            && let Some(world) = world
        {
            world.register_loaded_chunk_entities(pos, ChunkStatus::Full, pending_entities);
        }
    }

    /// Runs Full-load post-processing and returns the number of packed positions attempted.
    pub(crate) fn post_process_generation(&self) -> Result<usize, PostProcessGenerationError> {
        let postprocessing = {
            let Some(full) = self.try_full_chunk() else {
                return Err(PostProcessGenerationError::ChunkNotFull);
            };
            let world = full
                .get_level()
                .ok_or(PostProcessGenerationError::WorldUnavailable)?;
            full.take_postprocessing()
                .map(|postprocessing| (world, full.common().pos, full.min_y(), postprocessing))
        };

        let post_process_position_count =
            if let Some((world, pos, min_y, postprocessing)) = postprocessing {
                let position_count = postprocessing.iter().map(Vec::len).sum();
                FullChunkRef::post_process_generation(&world, pos, min_y, postprocessing);
                position_count
            } else {
                0
            };
        let Some(full) = self.try_full_chunk() else {
            return Err(PostProcessGenerationError::ChunkNotFull);
        };
        full.promote_pending_block_entities();
        Ok(post_process_position_count)
    }

    /// Finishes a generated status on the async scheduler after the Rayon task returns.
    fn finish_generation_status(self: &Arc<Self>, status: ChunkStatus) {
        if let Some(stored_chunk) = self.data.get()
            && self
                .published_status()
                .is_none_or(|published| published < status)
        {
            stored_chunk.mark_dirty();
        }

        if status == ChunkStatus::Full {
            self.register_full_chunk_ticks();
        }

        self.mark_status_work_published(status);
        self.publish_generated_status(status);

        if status == ChunkStatus::Full {
            self.publish_full();
        }
    }

    #[cfg(test)]
    pub(crate) fn finish_generation_status_for_test(self: &Arc<Self>, status: ChunkStatus) {
        self.finish_generation_status(status);
    }

    /// Inserts a chunk into the holder with a specific status.
    /// This notifies watchers - use `insert_chunk_no_notify` + separate notification
    /// if calling from a rayon thread to avoid contention.
    ///
    /// # Panics
    ///
    /// Panics if `status` claims Full without initialized Full runtime state, or if
    /// initialized Full runtime state is paired with a lower status.
    pub fn insert_chunk(self: &Arc<Self>, chunk: Chunk, status: ChunkStatus) {
        self.store_and_publish_chunk_status(chunk, status);
        if status == ChunkStatus::Full {
            self.publish_full();
        }
    }

    fn store_and_publish_chunk_status(&self, chunk: Chunk, status: ChunkStatus) {
        assert_eq!(
            self.published_status.load(Ordering::Acquire),
            UNPUBLISHED_STATUS,
            "initial chunk installation cannot replace published data"
        );
        assert_eq!(
            status == ChunkStatus::Full,
            chunk.full_runtime().is_some(),
            "initial chunk status must match its Full runtime state"
        );
        assert!(
            self.data.set(chunk).is_ok(),
            "initial chunk installation cannot replace existing data"
        );
        if status == ChunkStatus::Full {
            self.register_full_chunk_ticks();
        }
        self.mark_status_work_published(status);
        self.published_status
            .store(encoded_published_status(status), Ordering::Release);
        self.status_changed.notify_waiters();
    }

    fn publish_generated_status(&self, status: ChunkStatus) {
        let encoded = encoded_published_status(status);
        let previous = self.published_status.fetch_max(encoded, Ordering::Release);
        if previous < encoded {
            self.status_changed.notify_waiters();
        }
    }

    /// Registers tick queues before Full status becomes observable to watchers.
    fn register_full_chunk_ticks(&self) {
        let Some(chunk) = self.data.get() else {
            panic!("Full status must have installed chunk data");
        };
        let Some(_) = chunk.full_runtime() else {
            panic!("Full status must expose a Full chunk view");
        };
        let full = FullChunkRef::from_full_context(chunk);
        let Some(world) = full.get_level() else {
            // Focused holder tests construct chunks without a live world. Real
            // loaded/generated chunks always carry the WorldGenContext world.
            return;
        };
        if let Err(error) = world.register_full_chunk_ticks(full) {
            panic!("Full chunk scheduled-tick registration invariant failed: {error:?}");
        }
    }

    fn publish_full(self: &Arc<Self>) {
        let Some(full) = self.try_full_chunk() else {
            return;
        };
        let world = full.get_level();
        if let Some(world) = world {
            world.update_entity_chunk_visibility(self.pos, self.entity_visibility());
        }
        self.full_status_initialized.store(true, Ordering::Release);
        if let Some(publications) = self.full_publications.upgrade() {
            publications.publish(self);
        }
    }

    /// Inserts a chunk into the holder without notifying watchers.
    /// The caller is responsible for notifying via the completion channel.
    pub(crate) fn insert_chunk_no_notify(&self, chunk: Chunk) {
        assert!(
            self.data.set(chunk).is_ok(),
            "initial chunk installation cannot replace existing data"
        );
    }

    /// Wakes all `await_chunk` watchers without changing the chunk result.
    /// This allows waiting futures to re-check `is_status_disallowed` and bail
    /// out during chunk unload.
    pub fn wake_all_watchers(&self) {
        self.status_changed.notify_waiters();
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
    use std::{task::Poll, time::Duration as TestDuration};
    use tokio::time::sleep as test_sleep;

    use crate::behavior::init_behaviors;
    use crate::chunk::Chunk;
    use crate::chunk::section::{ChunkSection, Sections};
    use crate::test_support::fresh_test_world;
    use crate::world::tick_scheduler::TickPriority;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks, vanilla_fluids};

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

    fn test_proto_chunk(_status: ChunkStatus) -> Chunk {
        Chunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        )
    }

    #[test]
    fn insert_chunk_publishes_the_authoritative_status() {
        init_chunk_test_registry();
        let holder = test_holder();
        let proto = Chunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );

        holder.insert_chunk(proto, ChunkStatus::Light);

        let Some(chunk) = holder.try_chunk(ChunkStatus::Light) else {
            panic!("inserted chunk should be available at published status");
        };
        assert_eq!(holder.published_status(), Some(ChunkStatus::Light));
        assert!(chunk.full_runtime().is_none());
    }

    #[test]
    #[should_panic(expected = "initial chunk status must match its Full runtime state")]
    fn insert_chunk_rejects_full_status_for_proto_data() {
        init_chunk_test_registry();
        test_holder().insert_chunk(test_proto_chunk(ChunkStatus::Spawn), ChunkStatus::Full);
    }

    #[test]
    fn full_readiness_publication_waits_for_post_load_initialization() {
        init_chunk_test_registry();
        let publications = Arc::new(FullPublicationQueue::default());
        let holder = Arc::new(ChunkHolder::new_with_full_publications(
            ChunkPos::new(0, 0),
            ChunkTicketLevel::FULL_CHUNK,
            None,
            0,
            16,
            Arc::downgrade(&publications),
        ));
        let full = test_proto_chunk(ChunkStatus::Light);
        let _ = full.promote_to_full();

        holder.store_and_publish_chunk_status(full, ChunkStatus::Full);

        assert_eq!(holder.published_status(), Some(ChunkStatus::Full));
        assert!(!holder.is_full_status_initialized());
        assert!(publications.drain().is_empty());

        holder.publish_full();

        assert!(holder.is_full_status_initialized());
        assert_eq!(publications.drain().len(), 1);
    }

    #[test]
    fn generated_full_status_is_accessible_when_readiness_is_published() {
        init_chunk_test_registry();
        let holder = test_holder();
        holder.insert_chunk(test_proto_chunk(ChunkStatus::Light), ChunkStatus::Light);
        holder.upgrade_to_full();

        assert_eq!(holder.entity_visibility(), EntityVisibility::Hidden);
        assert!(!holder.is_full_status_initialized());

        holder.finish_generation_status(ChunkStatus::Full);

        assert_eq!(holder.entity_visibility(), EntityVisibility::Tracked);
        assert!(holder.is_full_status_initialized());
    }

    #[test]
    fn late_lower_generation_completion_does_not_regress_published_status() {
        init_chunk_test_registry();
        let holder = test_holder();
        holder.insert_chunk(test_proto_chunk(ChunkStatus::Light), ChunkStatus::Light);

        holder.finish_generation_status(ChunkStatus::Spawn);
        holder.finish_generation_status(ChunkStatus::Features);

        assert_eq!(holder.published_status(), Some(ChunkStatus::Spawn));
        assert!(holder.try_chunk(ChunkStatus::Spawn).is_some());
    }

    #[tokio::test]
    async fn status_waiter_observes_publication_after_subscribing() {
        init_chunk_test_registry();
        let holder = test_holder();
        let waiter = holder.await_chunk_status(ChunkStatus::Empty);

        holder.insert_chunk(test_proto_chunk(ChunkStatus::Empty), ChunkStatus::Empty);

        assert_eq!(waiter.await, Some(ChunkStatus::Empty));
    }

    #[tokio::test]
    async fn pending_status_waiters_wake_after_publication() {
        init_chunk_test_registry();
        let holder = test_holder();
        let first_waiter = holder.await_chunk_status(ChunkStatus::Empty);
        let second_waiter = holder.await_chunk_status(ChunkStatus::Empty);
        tokio::pin!(first_waiter, second_waiter);
        assert!(matches!(futures::poll!(&mut first_waiter), Poll::Pending));
        assert!(matches!(futures::poll!(&mut second_waiter), Poll::Pending));

        let publishing_holder = Arc::clone(&holder);
        let publish_task = tokio::spawn(async move {
            publishing_holder
                .insert_chunk(test_proto_chunk(ChunkStatus::Empty), ChunkStatus::Empty);
        });

        let (first_status, second_status) = tokio::select! {
            biased;
            () = test_sleep(TestDuration::from_secs(1)) => {
                panic!("pending status waiters were not woken by publication");
            }
            statuses = async { tokio::join!(&mut first_waiter, &mut second_waiter) } => statuses,
        };
        assert_eq!(first_status, Some(ChunkStatus::Empty));
        assert_eq!(second_status, Some(ChunkStatus::Empty));
        assert!(publish_task.await.is_ok());
    }

    #[test]
    fn full_registration_transfers_prepublication_block_and_fluid_ticks() {
        init_chunk_test_registry();
        let world = fresh_test_world("prepublication_tick_transfer");
        let chunk_pos = ChunkPos::new(0, 0);
        let min_y = world.get_min_y();
        let height = world.get_height();
        let sections = (0..height / 16)
            .map(|_| ChunkSection::new_empty())
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let proto = Chunk::new(
            Sections::from_owned(sections),
            chunk_pos,
            min_y,
            height,
            Arc::downgrade(&world),
        );
        let block_pos = BlockPos::new(1, min_y + 1, 1);
        let fluid_pos = BlockPos::new(2, min_y + 1, 2);
        proto.schedule_block_tick(block_pos, &vanilla_blocks::STONE, TickPriority::High);
        proto.schedule_fluid_tick(fluid_pos, &vanilla_fluids::WATER, TickPriority::Low);

        let holder = Arc::new(ChunkHolder::new(
            chunk_pos,
            ChunkTicketLevel::FULL_CHUNK,
            Some(ChunkTicketLevel::FULL_CHUNK),
            min_y,
            height,
        ));
        let _ = world
            .chunk_map
            .chunks
            .insert_sync(chunk_pos, Arc::clone(&holder));
        holder.insert_chunk(proto, ChunkStatus::Light);
        holder.upgrade_to_full();

        assert!(!world.has_registered_full_chunk_ticks(chunk_pos));
        holder.finish_generation_status(ChunkStatus::Full);

        assert!(world.has_registered_full_chunk_ticks(chunk_pos));
        assert!(world.has_scheduled_block_tick(block_pos, &vanilla_blocks::STONE));
        assert!(world.has_scheduled_fluid_tick(fluid_pos, &vanilla_fluids::WATER));
    }

    #[test]
    fn client_deltas_require_confirmed_block_readiness() {
        init_chunk_test_registry();
        let holder = test_holder();
        let full = test_proto_chunk(ChunkStatus::Light);
        let _ = full.promote_to_full();
        holder.insert_chunk(full, ChunkStatus::Full);
        let pos = BlockPos::new(1, 1, 1);
        let section_pos = SectionPos::new(0, 0, 0);
        let revision = holder.packet_content_revision();
        let chunk = holder
            .try_chunk(ChunkStatus::Full)
            .expect("the test holder should contain a Full chunk");
        chunk.clear_dirty();

        assert!(!holder.block_changed(pos));
        assert!(!holder.light_changed(LightLayer::Block, section_pos));
        assert_eq!(holder.packet_content_revision(), revision);
        assert!(
            holder
                .try_chunk(ChunkStatus::Full)
                .is_some_and(Chunk::is_dirty),
            "pre-readiness light changes must still be persisted"
        );

        holder.transition_ticking_readiness(TickingReadiness::BlockTicking);

        assert!(holder.light_changed(LightLayer::Block, section_pos));
        assert_eq!(holder.packet_content_revision(), revision + 1);
        holder.clear_broadcast_queued();
        assert!(holder.block_changed(pos));
        assert_eq!(holder.packet_content_revision(), revision + 2);
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
        holder.insert_chunk(test_proto_chunk(ChunkStatus::Empty), ChunkStatus::Empty);

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
            test_proto_chunk(ChunkStatus::StructureStarts),
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

    #[test]
    fn save_dependency_controls_ready_for_saving() {
        let holder = test_holder();
        assert!(holder.is_ready_for_saving());

        let first = holder.add_save_dependency();
        let second = holder.add_save_dependency();
        assert!(!holder.is_ready_for_saving());

        drop(first);
        assert!(!holder.is_ready_for_saving());

        drop(second);
        assert!(holder.is_ready_for_saving());
    }

    #[test]
    fn save_preparation_defers_revival_only_until_the_snapshot_is_built() {
        let holder = test_holder();
        holder.begin_unloading();
        let preparation = holder
            .try_begin_save_preparation()
            .expect("an unloading holder should begin save preparation");

        assert!(!holder.try_revive_from_unloading());

        drop(preparation);

        assert!(holder.try_revive_from_unloading());
        assert!(holder.try_begin_save_preparation().is_none());
    }

    #[test]
    fn revival_winning_the_lifecycle_race_cancels_save_preparation() {
        let holder = test_holder();
        holder.begin_unloading();

        assert!(holder.try_revive_from_unloading());
        assert!(holder.try_begin_save_preparation().is_none());
    }
}
