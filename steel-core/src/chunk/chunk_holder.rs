//! `ChunkHolder` manages chunk state and asynchronous generation tasks.
use futures::Future;
use parking_lot::RwLockReadGuard;
use rustc_hash::FxHashSet;
use std::fmt::Debug;
use std::mem;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use steel_utils::locks::SyncRwLock;
use steel_utils::{BlockPos, ChunkPos, SectionPos, locks::SyncMutex};
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
use crate::chunk::chunk_ticket_manager::generation_status;
use crate::world::World;
use crate::{
    ChunkMap,
    chunk::{
        chunk_access::{ChunkAccess, ChunkStatus},
        chunk_generation_task::ChunkGenerationTask,
        chunk_pyramid::ChunkStep,
        level_chunk::LevelChunk,
    },
};

const STATUS_NONE: u8 = u8::MAX;

/// The result of a chunk operation.
pub enum ChunkResult {
    /// The chunk is not loaded.
    Unloaded,
    /// The chunk operation failed.
    Failed,
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
/// `ChunkResult::Failed` -> data is Anything and should not be used anymore
///
/// `ChunkResult::Ok(status except Full)` -> data is `Some(ChunkAccess::Proto(ProtoChunk))`
///
/// `ChunkResult::Ok(ChunkStatus::Full)` -> data is `Some(ChunkAccess::Full(LevelChunk))`
pub struct ChunkHolder {
    data: ChunkGuard,
    chunk_result: watch::Receiver<ChunkResult>,
    sender: watch::Sender<ChunkResult>,
    generation_task: SyncMutex<Option<Arc<ChunkGenerationTask>>>,
    pos: ChunkPos,
    /// The current ticket level of the chunk.
    pub ticket_level: AtomicU8,
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
    /// Per-section sets of changed block positions (section-relative packed shorts).
    /// Index is `(block_y - min_y) / 16`.
    changed_blocks_per_section: Box<[SyncMutex<FxHashSet<i16>>]>,
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
    pub fn new(pos: ChunkPos, ticket_level: u8, min_y: i32, height: i32) -> Self {
        let (sender, receiver) = watch::channel(ChunkResult::Unloaded);
        let highest_allowed_status =
            generation_status(Some(ticket_level)).map_or(STATUS_NONE, |s| s.get_index() as u8);

        let section_count = (height / 16) as usize;
        let changed_blocks_per_section: Box<[SyncMutex<FxHashSet<i16>>]> = (0..section_count)
            .map(|_| SyncMutex::new(FxHashSet::default()))
            .collect();

        Self {
            data: ChunkGuard::new(ChunkAccess::Unloaded),
            chunk_result: receiver,
            sender,
            generation_task: SyncMutex::new(None),
            pos,
            ticket_level: AtomicU8::new(ticket_level),
            started_work: AtomicUsize::new(usize::MAX),
            highest_allowed_status: AtomicU8::new(highest_allowed_status),
            min_y,
            height,
            has_changed_sections: AtomicBool::new(false),
            changed_blocks_per_section,
        }
    }

    /// Updates the highest allowed generation status based on the ticket level.
    pub fn update_highest_allowed_status(&self, ticket_level: u8) {
        let new_status =
            generation_status(Some(ticket_level)).map_or(STATUS_NONE, |s| s.get_index() as u8);
        self.highest_allowed_status
            .store(new_status, Ordering::Release);
    }

    /// Records a block change at the given position.
    /// Returns `true` if this is the first change (chunk should be added to broadcast list).
    pub fn block_changed(&self, pos: &BlockPos) -> bool {
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
    pub fn take_changed_blocks(&self) -> Vec<(usize, FxHashSet<i16>)> {
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
    #[allow(clippy::missing_panics_doc)]
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

        let task = self.generation_task.lock();

        #[allow(clippy::unwrap_used)]
        if task.is_none() || status > task.as_ref().unwrap().target_status {
            drop(task);
            self.reschedule_chunk_task_b(status, chunk_map);
            true
        } else {
            false
        }
    }

    /// Reschedules the chunk task to the given status.
    #[inline]
    pub(crate) fn reschedule_chunk_task_b(&self, status: ChunkStatus, chunk_map: &Arc<ChunkMap>) {
        let new_task = chunk_map.schedule_generation_task_b(status, self.pos);
        let mut old_task_guard = self.generation_task.lock();

        let old_task = old_task_guard.replace(new_task);
        drop(old_task_guard);

        if let Some(old_task) = old_task {
            old_task.mark_for_cancel();
        }
    }

    /// Gets access to the chunk if it has reached the given status.
    #[inline]
    pub fn try_chunk(&self, status: ChunkStatus) -> Option<RwLockReadGuard<'_, ChunkAccess>> {
        match &*self.chunk_result.borrow() {
            ChunkResult::Ok(s) if status <= *s => Some(self.data.read()),
            _ => None,
        }
    }

    /// Waits until the chunk has reached the given status, then calls the function.
    pub fn await_chunk(
        &self,
        status: ChunkStatus,
    ) -> impl Future<Output = Option<RwLockReadGuard<'_, ChunkAccess>>> {
        let mut subscriber = self.sender.subscribe();
        async move {
            loop {
                {
                    let chunk_result = subscriber.borrow_and_update();
                    match &*chunk_result {
                        ChunkResult::Ok(s) if status <= *s => {
                            return Some(self.data.read());
                        }
                        ChunkResult::Failed => {
                            return None;
                        }
                        _ => {}
                    }
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

    /// Gets the persisted status of the chunk.
    pub fn persisted_status(&self) -> Option<ChunkStatus> {
        match &*self.chunk_result.borrow() {
            ChunkResult::Ok(s) => Some(*s),
            _ => None,
        }
    }

    /// Applies a step to the chunk.
    ///
    /// # Panics
    /// Panics if the target status is not Empty and has no parent, or if the chunk status is invalid during generation.
    #[allow(clippy::too_many_lines)]
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

        if !self.acquire_status_bump(target_status) {
            let self_clone = self.clone();
            return Some(Box::pin(async move {
                self_clone.await_chunk(target_status).await.map(|_| ())
            }));
        }

        let sender = self.sender.clone();
        let cache = cache.clone();
        let context = chunk_map.world_gen_context.clone();
        // This is one of the `crate::chunk::chunk_status_tasks` functions.
        let task = step.task;
        let self_clone = self.clone();
        let storage = chunk_map.storage.clone();

        let future = chunk_map.task_tracker.spawn(async move {
            if target_status == ChunkStatus::Empty {
                // Acquire the region first (creates if needed, increments ref count)
                let chunk_exists = storage.acquire_chunk(self_clone.pos).await.unwrap_or(false);

                if chunk_exists {
                    // Try to load the chunk from disk
                    if let Ok(Some((chunk, status))) = storage
                        .load_chunk(
                            self_clone.pos,
                            self_clone.min_y(),
                            self_clone.height(),
                            context.weak_world(),
                        )
                        .await
                    {
                        self_clone.insert_chunk(chunk, status);
                    } else {
                        // Chunk existed but failed to load - generate fresh
                        let holder_for_notify = self_clone.clone();
                        rayon_spawn(&thread_pool, move || {
                            task(context, step, &cache, self_clone)
                        })
                        .await
                        .expect("Should never fail creating an empty chunk");
                        holder_for_notify.notify_status(target_status);
                    }
                } else {
                    // Chunk doesn't exist - generate fresh
                    let holder_for_notify = self_clone.clone();
                    rayon_spawn(&thread_pool, move || {
                        task(context, step, &cache, self_clone)
                    })
                    .await
                    .expect("Should never fail creating an empty chunk");
                    holder_for_notify.notify_status(target_status);
                }
                #[cfg(feature = "slow_chunk_gen")]
                if SLOW_CHUNK_GEN.load(Ordering::Relaxed) {
                    sleep(Duration::from_millis(200)).await;
                }
                Some(())
            } else {
                let parent_status = target_status
                    .parent()
                    .expect("Target status must have parent if not Empty");

                //log::info!(
                //    "Parent status: {:?}, target status: {:?}",
                //    parent_status,
                //    target_status
                //);

                let has_parent = self_clone.try_chunk(parent_status).is_some();
                let self_clone2 = self_clone.clone();

                assert!(has_parent, "Parent chunk missing");

                match rayon_spawn(&thread_pool, move || {
                    task(context, step, &cache, self_clone)
                })
                .await
                {
                    Ok(()) => {
                        sender.send_modify(|chunk| {
                            // Update inner status
                            if let ChunkAccess::Proto(chunk) = &*self_clone2.data.read() {
                                chunk.set_status(target_status);
                            }
                            if let ChunkResult::Ok(s) = chunk {
                                if *s < target_status {
                                    *s = target_status;
                                } else if *s != ChunkStatus::Full {
                                    // Means it advanced a loaded chunk
                                }
                            }
                        });
                        #[cfg(feature = "slow_chunk_gen")]
                        if SLOW_CHUNK_GEN.load(Ordering::Relaxed) {
                            sleep(Duration::from_millis(200)).await;
                        }
                        Some(())
                    }
                    Err(e) => {
                        log::error!("Chunk generation task failed: {e}");
                        sender.send_replace(ChunkResult::Failed);
                        None
                    }
                }
            }
        });

        Some(Box::pin(async move {
            match future.await {
                Ok(result) => result,
                Err(e) => {
                    log::error!("Chunk generation task failed: {e}");
                    None
                }
            }
        }))
    }

    fn acquire_status_bump(&self, status: ChunkStatus) -> bool {
        let status_index = status.get_index();
        let parent_index = status
            .parent()
            .map_or(usize::MAX, super::chunk_access::ChunkStatus::get_index);

        //log::info!(
        //    "Parent index: {:?}, Status index: {:?}",
        //    parent_index,
        //    status_index
        //);

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
        self.data.with_write(|chunk| {
            use std::mem::replace;
            let owned = replace(chunk, ChunkAccess::Unloaded);

            *chunk = match owned {
                ChunkAccess::Proto(proto) => {
                    let min_y = proto.min_y();
                    let height = proto.height();
                    ChunkAccess::Full(LevelChunk::from_proto(proto, min_y, height, level))
                }
                ChunkAccess::Full(full) => ChunkAccess::Full(full),
                ChunkAccess::Unloaded => panic!("Chunk is unloaded, cannot upgrade to full"),
            };
        });
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

    /// Notifies watchers that the chunk has reached a status.
    /// Called by the drainer task after `insert_chunk_no_notify`.
    pub fn notify_status(&self, status: ChunkStatus) {
        self.sender.send_replace(ChunkResult::Ok(status));
    }

    /// Cancels the current generation task.
    pub fn cancel_generation_task(&self) {
        let mut task_guard = self.generation_task.lock();
        if let Some(task) = task_guard.take() {
            task.mark_for_cancel();
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
