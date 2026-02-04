use std::{
    io, mem,
    sync::{
        Arc, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use rayon::{
    ThreadPool, ThreadPoolBuilder,
    iter::{IntoParallelIterator, ParallelIterator},
};
use rustc_hash::FxBuildHasher;
use steel_protocol::packets::game::{
    BlockChange, CBlockUpdate, CSectionBlocksUpdate, CSetChunkCenter,
};
use steel_registry::{REGISTRY, dimension_type::DimensionTypeRef, vanilla_blocks};
use steel_utils::{BlockPos, ChunkPos, SectionPos, locks::SyncMutex};
use tokio::runtime::Runtime;
use tokio_util::task::TaskTracker;
use tracing::instrument;

use crate::chunk::chunk_holder::ChunkHolder;
use crate::chunk::chunk_ticket_manager::{
    ChunkTicketManager, LevelChange, MAX_VIEW_DISTANCE, is_full,
};
use crate::chunk::player_chunk_view::PlayerChunkView;
use crate::chunk::world_gen_context::ChunkGeneratorType;
use crate::chunk::{chunk_access::ChunkAccess, chunk_ticket_manager::is_ticked};
use crate::chunk::{
    chunk_access::ChunkStatus, chunk_generation_task::ChunkGenerationTask,
    flat_chunk_generator::FlatChunkGenerator, world_gen_context::WorldGenContext,
};
use crate::chunk_saver::RegionManager;
use crate::player::Player;
use crate::world::World;

/// Timing information for chunk map tick operations.
#[derive(Debug, Default)]
pub struct ChunkMapTickTimings {
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
    /// Time spent broadcasting block changes.
    pub broadcast_changes: Duration,
    /// Time spent processing chunk unloads.
    pub process_unloads: Duration,
    /// Time spent collecting tickable chunks.
    pub collect_tickable: Duration,
    /// Time spent ticking chunks (random ticks, etc.).
    pub tick_chunks: Duration,
    /// Number of chunks that were ticked.
    pub tickable_count: usize,
    /// Total number of loaded chunks.
    pub total_chunks: usize,
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
    /// Manager for chunk saving and loading.
    pub region_manager: Arc<RegionManager>,
    /// Chunk holders with pending block changes to broadcast.
    pub chunks_to_broadcast: SyncMutex<Vec<Arc<ChunkHolder>>>,
    /// Last length of `tickable_chunks` to pre-allocate with appropriate capacity.
    last_tickable_len: AtomicUsize,
}

impl ChunkMap {
    /// Creates a new chunk map.
    #[must_use]
    #[allow(clippy::missing_panics_doc, clippy::unwrap_used)]
    pub fn new(
        chunk_runtime: Arc<Runtime>,
        world: Weak<World>,
        dimension: &DimensionTypeRef,
    ) -> Self {
        let generator = Arc::new(ChunkGeneratorType::Flat(FlatChunkGenerator::new(
            REGISTRY
                .blocks
                .get_default_state_id(vanilla_blocks::BEDROCK), // Bedrock
            REGISTRY.blocks.get_default_state_id(vanilla_blocks::DIRT), // Dirt
            REGISTRY
                .blocks
                .get_default_state_id(vanilla_blocks::GRASS_BLOCK), // Grass Block
        )));

        Self {
            chunks: scc::HashMap::default(),
            unloading_chunks: scc::HashMap::default(),
            pending_generation_tasks: SyncMutex::new(Vec::new()),
            task_tracker: TaskTracker::new(),
            chunk_tickets: SyncMutex::new(ChunkTicketManager::new()),
            world_gen_context: Arc::new(WorldGenContext::new(generator, world)),
            generation_pool: Arc::new(ThreadPoolBuilder::new().build().unwrap()),
            //tick_pool: Arc::new(ThreadPoolBuilder::new().build().unwrap()),
            chunk_runtime,
            region_manager: Arc::new(RegionManager::new(format!("world/{}", dimension.key.path))),
            chunks_to_broadcast: SyncMutex::new(Vec::new()),
            last_tickable_len: AtomicUsize::new(0),
        }
    }

    /// Executes a function with access to a fully loaded chunk.
    /// Returns `None` if the chunk is not loaded or not at Full status.
    #[allow(clippy::missing_panics_doc)]
    pub fn with_full_chunk<F, R>(&self, pos: &ChunkPos, f: F) -> Option<R>
    where
        F: FnOnce(&ChunkAccess) -> R,
    {
        let chunk_holder = self.chunks.read_sync(pos, |_, chunk| chunk.clone())?;
        let guard = chunk_holder.try_chunk(ChunkStatus::Full)?;
        Some(f(&guard))
    }

    /// Records a block change at the given position.
    /// This marks the chunk as having pending changes to broadcast.
    pub fn block_changed(&self, pos: &BlockPos) {
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

    /// Broadcasts all pending block changes to nearby players.
    ///
    /// # Panics
    /// Panics if a section has exactly one change (should never happen).
    pub fn broadcast_changed_chunks(&self) {
        let holders = {
            let mut guard = self.chunks_to_broadcast.lock();
            if guard.is_empty() {
                return;
            }
            mem::take(&mut *guard)
        };

        let world = self.world_gen_context.world();

        for holder in holders {
            let chunk_pos = holder.get_pos();
            let min_y = holder.min_y();

            // Take all pending changes from this chunk holder
            let changes_by_section = holder.take_changed_blocks();

            if changes_by_section.is_empty() {
                continue;
            }

            // Get players tracking this chunk
            let tracking_players = world.player_area_map.get_tracking_players(chunk_pos);
            if tracking_players.is_empty() {
                continue;
            }

            // For each section with changes, send appropriate packet
            for (section_index, changed_positions) in changes_by_section {
                let section_y = min_y / 16 + section_index as i32;
                let section_pos = SectionPos::new(chunk_pos.0.x, section_y, chunk_pos.0.y);

                if changed_positions.len() == 1 {
                    // Single block change - use CBlockUpdate
                    let packed = *changed_positions.iter().next().expect("len == 1");
                    let block_pos = section_pos.relative_to_block_pos(packed);
                    let block_state = world.get_block_state(&block_pos);

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

                    for entity_id in &tracking_players {
                        if let Some(player) = world.players.get_by_entity_id(*entity_id) {
                            player.connection.send_packet(update_packet.clone());
                        }
                    }
                } else {
                    // Multiple block changes - use CSectionBlocksUpdate
                    let changes: Vec<BlockChange> = changed_positions
                        .iter()
                        .map(|&packed| {
                            let block_pos = section_pos.relative_to_block_pos(packed);
                            let block_state = world.get_block_state(&block_pos);
                            BlockChange {
                                pos: block_pos,
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

                    for entity_id in &tracking_players {
                        if let Some(player) = world.players.get_by_entity_id(*entity_id) {
                            player.connection.send_packet(packet.clone());
                        }
                    }
                }
            }
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
        ));
        self.pending_generation_tasks.lock().push(task.clone());
        task
    }

    /// Runs queued generation tasks.
    #[instrument(level = "trace", skip(self))]
    pub fn run_generation_tasks_b(&self) {
        let mut pending = self.pending_generation_tasks.lock();
        if pending.is_empty() {
            return;
        }
        let task_count = pending.len();
        tracing::trace!(task_count, "Running generation tasks");
        let tasks = pending.drain(..).collect::<Vec<_>>();
        drop(pending); // Release lock before spawning

        for task in tasks {
            self.task_tracker
                .spawn_on(async move { task.run().await }, self.chunk_runtime.handle());
        }
    }

    /// Updates scheduling for a chunk based on its new level.
    /// Returns the chunk holder if it is active.
    #[inline]
    #[allow(clippy::missing_panics_doc, clippy::unwrap_used)]
    pub fn update_chunk_level(
        self: &Arc<Self>,
        pos: &ChunkPos,
        new_level: Option<u8>,
    ) -> Option<Arc<ChunkHolder>> {
        // Recover from unloading if possible, else create new holder.
        let chunk_holder =
            if let Some(holder) = self.chunks.read_sync(pos, |_, holder| holder.clone()) {
                holder
            } else {
                new_level?;

                if let Some(entry) = self.unloading_chunks.remove_sync(pos) {
                    let _ = self.chunks.insert_sync(*pos, entry.1.clone());
                    entry.1
                } else {
                    let holder = Arc::new(ChunkHolder::new(
                        *pos,
                        new_level.unwrap(),
                        self.world_gen_context.min_y(),
                        self.world_gen_context.height(),
                    ));
                    let _ = self.chunks.insert_sync(*pos, holder.clone());
                    holder
                }
            };

        if let Some(level) = new_level {
            let old = chunk_holder.ticket_level.swap(level, Ordering::Relaxed);
            if old != level {
                chunk_holder.update_highest_allowed_status(level);
            }
            Some(chunk_holder)
        } else {
            //log::info!("Unloading chunk at {pos:?}");
            chunk_holder.cancel_generation_task();
            chunk_holder.ticket_level.store(u8::MAX, Ordering::Relaxed);
            chunk_holder.update_highest_allowed_status(u8::MAX);

            // Move to unloading_chunks for deferred unload
            if let Some((_, holder)) = self.chunks.remove_sync(pos) {
                let _ = self.unloading_chunks.insert_sync(*pos, holder);
            }
            None
        }
    }

    /// Processes chunk updates and ticks chunks.
    ///
    /// # Arguments
    /// * `tick_count` - The current server tick count
    /// * `random_tick_speed` - Number of random blocks to tick per section per tick
    /// * `runs_normally` - Whether game elements should run (false when frozen)
    ///
    /// Returns timing information for each phase of the tick.
    #[allow(clippy::too_many_lines)]
    #[instrument(level = "trace", skip(self), name = "chunk_map_tick")]
    pub fn tick_b(
        self: &Arc<Self>,
        tick_count: u64,
        random_tick_speed: u32,
        runs_normally: bool,
    ) -> ChunkMapTickTimings {
        let mut timings = ChunkMapTickTimings::default();

        {
            let mut ct = self.chunk_tickets.lock();

            // Only process chunks that actually changed
            let changes: Vec<LevelChange> = {
                let _span = tracing::trace_span!("ticket_updates").entered();
                let start = Instant::now();
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
                        self.update_chunk_level(&change.pos, change.new_level)
                            .map(|holder| (holder, change.new_level))
                    })
                    .collect();
                timings.holder_creation = start.elapsed();
                result
            };

            {
                let _span = tracing::trace_span!("schedule_generation").entered();
                let start = Instant::now();
                let scheduled_count: usize = holders_to_schedule
                    .into_par_iter()
                    .filter(|(holder, level)| {
                        level.is_some_and(is_full)
                            && holder.schedule_chunk_generation_task_b(ChunkStatus::Full, self)
                    })
                    .count();
                timings.schedule_generation = start.elapsed();
                timings.scheduled_count = scheduled_count;
            }
        };

        {
            let _span = tracing::trace_span!("run_generation").entered();
            let start = Instant::now();
            self.run_generation_tasks_b();
            timings.run_generation = start.elapsed();
        }

        {
            let _span = tracing::trace_span!("broadcast_changes").entered();
            let start = Instant::now();
            self.broadcast_changed_chunks();
            timings.broadcast_changes = start.elapsed();
        }

        {
            let _span = tracing::trace_span!("process_unloads").entered();
            let start = Instant::now();
            self.process_unloads();
            timings.process_unloads = start.elapsed();
        }

        if tick_count.is_multiple_of(100) {
            tracing::debug!(
                chunks = self.chunks.len(),
                unloading = self.unloading_chunks.len(),
                "Chunk map status"
            );
        }

        // Chunk ticking - skip when frozen
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
                let level = holder.ticket_level.load(Ordering::Relaxed);
                if is_ticked(level) {
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
                // TODO: In the future we might want to tick different regions/islands in parallel
                for holder in &tickable_chunks {
                    if let Some(chunk_guard) = holder.try_chunk(ChunkStatus::Full) {
                        chunk_guard.tick(random_tick_speed, tick_count as i32);
                    }
                }
                timings.tick_chunks = start.elapsed();
            }
        }

        timings
    }

    /// Saves a chunk to disk. Does not remove from `unloading_chunks`.
    #[allow(clippy::missing_panics_doc, clippy::unwrap_used)]
    #[instrument(level = "trace", skip(self, chunk_holder), fields(chunk = ?chunk_holder.get_pos()))]
    async fn save_chunk(&self, chunk_holder: &Arc<ChunkHolder>) {
        // Prepare chunk data while holding the lock, then release before async I/O
        let prepared = {
            let Some(chunk_guard) = chunk_holder.try_chunk(ChunkStatus::StructureStarts) else {
                // Chunk was at Empty stage so no need to save it
                return;
            };

            let status = chunk_holder
                .persisted_status()
                .expect("The check above confirmed it exists");

            let prepared = RegionManager::prepare_chunk_save(&chunk_guard);

            // Clear dirty flag while we still have the lock (only if we're actually saving)
            if prepared.is_some() {
                chunk_guard.clear_dirty();
            }

            (prepared, status)
        }; // chunk_guard dropped here

        let (prepared, status) = prepared;

        // Save chunk data if dirty
        if let Some(prepared) = prepared {
            let result = self.region_manager.save_chunk_data(prepared, status).await;

            if let Err(e) = result {
                tracing::error!("Error saving chunk: {e}");
            }
        }
    }

    /// Processes chunks that are pending unload.
    ///
    /// Iterates over `unloading_chunks`. For each chunk with `strong_count == 1`:
    /// - If dirty: spawn save task (keep until saved and clean)
    /// - If not dirty: release region handle and remove
    #[instrument(level = "trace", skip(self))]
    pub fn process_unloads(self: &Arc<Self>) {
        self.unloading_chunks.retain_sync(|pos, holder| {
            if Arc::strong_count(holder) == 1 {
                // Check if dirty by trying to get chunk access
                let is_dirty = holder
                    .try_chunk(ChunkStatus::StructureStarts)
                    .is_some_and(|chunk| chunk.is_dirty());

                if is_dirty {
                    // Save the chunk, keep until next tick when it's clean
                    let holder_clone = holder.clone();
                    let map_clone = self.clone();
                    self.task_tracker.spawn(async move {
                        map_clone.save_chunk(&holder_clone).await;
                    });
                    true // keep until clean
                } else {
                    // Clean and no refs - release region handle and remove
                    let pos = *pos;
                    let map_clone = self.clone();
                    self.task_tracker.spawn(async move {
                        if let Err(e) = map_clone.region_manager.release_chunk(pos).await {
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
        let current_chunk_pos = *player.last_chunk_pos.lock();
        let view_distance = player.view_distance();

        let new_view = PlayerChunkView::new(current_chunk_pos, view_distance);
        let mut last_view_guard = player.last_tracking_view.lock();

        if last_view_guard.as_ref() != Some(&new_view) {
            let mut chunk_tickets = self.chunk_tickets.lock();

            let connection = &player.connection;
            let world = self.world_gen_context.world();

            if let Some(last_view) = last_view_guard.as_ref() {
                if last_view.center != new_view.center
                    || last_view.view_distance != new_view.view_distance
                {
                    chunk_tickets.remove_ticket(
                        last_view.center,
                        MAX_VIEW_DISTANCE.saturating_sub(last_view.view_distance),
                    );
                    chunk_tickets.add_ticket(
                        new_view.center,
                        MAX_VIEW_DISTANCE.saturating_sub(new_view.view_distance),
                    );

                    connection.send_packet(CSetChunkCenter {
                        x: new_view.center.0.x,
                        y: new_view.center.0.y,
                    });
                }

                // Track chunks for PlayerAreaMap update
                let mut added_chunks = Vec::new();
                let mut removed_chunks = Vec::new();

                // We lock here to ensure we have unique access for the duration of the diff
                let mut chunk_sender = player.chunk_sender.lock();
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
                    player.id,
                    &added_chunks,
                    &removed_chunks,
                );

                // Update entity tracking for this player (only check added/removed chunks)
                world.entity_tracker().on_player_view_change(
                    player,
                    &added_chunks,
                    &removed_chunks,
                );
            } else {
                chunk_tickets.add_ticket(
                    new_view.center,
                    MAX_VIEW_DISTANCE.saturating_sub(new_view.view_distance),
                );

                // Send initial chunk cache center to client
                connection.send_packet(CSetChunkCenter {
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

                // Initial entity tracking for this player
                world.entity_tracker().on_player_join(player, &new_view);
            }

            *last_view_guard = Some(new_view);
        }
    }

    /// Removes a player from the chunk map.
    pub fn remove_player(&self, player: &Player) {
        // Okay to lock sync lock here cause it has low contention
        let mut last_view_guard = player.last_tracking_view.lock();
        if let Some(last_view) = last_view_guard.take() {
            drop(last_view_guard);
            let mut chunk_tickets = self.chunk_tickets.lock();
            chunk_tickets.remove_ticket(
                last_view.center,
                MAX_VIEW_DISTANCE.saturating_sub(last_view.view_distance),
            );
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

        tracing::info!(chunk_count = all_chunks.len(), "Saving chunks");

        // Save all chunks that have data
        for holder in &all_chunks {
            let prepared = {
                let Some(chunk) = holder.try_chunk(ChunkStatus::StructureStarts) else {
                    continue;
                };
                let Some(status) = holder.persisted_status() else {
                    continue;
                };
                let Some(prepared) = RegionManager::prepare_chunk_save(&chunk) else {
                    continue; // Not dirty
                };
                chunk.clear_dirty();
                (prepared, status)
            };

            let (prepared, status) = prepared;
            match self.region_manager.save_chunk_data(prepared, status).await {
                Ok(true) => saved_count += 1,
                Ok(false) => {} // Not dirty
                Err(e) => {
                    tracing::error!(chunk = ?holder.get_pos(), "Failed to save chunk: {e}");
                }
            }
        }

        // Close all region files (flushes headers and releases file handles)
        if let Err(e) = self.region_manager.close_all().await {
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
