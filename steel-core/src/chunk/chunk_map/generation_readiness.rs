use super::{
    Arc, ChunkGenerationTask, ChunkHolder, ChunkMap, ChunkPos, ChunkStatus, ChunkTicketLevel,
    DeferredChunkRevival, FullNeighborhoodCounts, FullNeighborhoodError, FullNeighborhoodIndex,
    FullPublication, FxHashMap, FxHashSet, GENERATION_THREAD_MULTIPLE, GenerationTaskPriority,
    Instant, LevelChange, Ordering, PackedChunkPos, PostProcessGenerationError,
    ReadinessReconcileResult, RunningGenerationTaskPermit, TickableChunk, TickingChunkSnapshot,
    TickingReadiness, TickingReadinessCandidate, instrument, is_block_ticking, is_entity_ticking,
    is_full,
};

impl ChunkMap {
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

    pub(super) fn max_running_generation_tasks(&self) -> usize {
        self.generation_pool.current_num_threads().max(1) * GENERATION_THREAD_MULTIPLE
    }

    pub(super) fn generation_task_priority(task: &ChunkGenerationTask) -> GenerationTaskPriority {
        let holder = task.center_holder();
        GenerationTaskPriority::for_levels(holder.load_level(), holder.simulation_level())
    }

    /// Updates scheduling for a chunk based on its new level.
    /// Returns the chunk holder if it is active.
    #[inline]
    pub(super) fn update_chunk_level(
        self: &Arc<Self>,
        pos: ChunkPos,
        new_level: Option<ChunkTicketLevel>,
        new_simulation_level: Option<ChunkTicketLevel>,
    ) -> Option<Arc<ChunkHolder>> {
        if new_level.is_none() {
            self.deferred_revivals.lock().remove(&pos);
        }

        // Recover from unloading if possible, else create new holder.
        let chunk_holder =
            if let Some(holder) = self.chunks.read_sync(&pos, |_, holder| holder.clone()) {
                holder
            } else {
                let level = new_level?;

                if let Some(entry) = self.unloading_chunks.remove_sync(&pos) {
                    let holder = entry.1;
                    if !holder.try_revive_from_unloading() {
                        let _ = self.unloading_chunks.insert_sync(pos, Arc::clone(&holder));
                        self.deferred_revivals.lock().insert(
                            pos,
                            DeferredChunkRevival {
                                load_level: level,
                                simulation_level: new_simulation_level,
                            },
                        );
                        return None;
                    }
                    let _ = self.chunks.insert_sync(pos, Arc::clone(&holder));
                    holder
                } else {
                    let holder = Arc::new(ChunkHolder::new_with_full_publications(
                        pos,
                        level,
                        new_simulation_level,
                        self.world_gen_context.min_y(),
                        self.world_gen_context.height(),
                        Arc::downgrade(&self.full_publications),
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
            if is_full(level)
                && !old.is_some_and(is_full)
                && chunk_holder.is_full_status_initialized()
                && chunk_holder.published_status() == Some(ChunkStatus::Full)
                && chunk_holder.try_chunk(ChunkStatus::Full).is_some()
            {
                self.full_publications.publish(&chunk_holder);
            }
            Some(chunk_holder)
        } else {
            //log::info!("Unloading chunk at {pos:?}");
            chunk_holder.begin_unloading();
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

            if let Some(chunk) = chunk_holder.try_full_chunk() {
                chunk.suspend_block_entities(&chunk_holder);
            }

            // Move to unloading_chunks for deferred unload
            if let Some((_, holder)) = self.chunks.remove_sync(&pos) {
                let _ = self.unloading_chunks.insert_sync(pos, holder);
            }
            None
        }
    }

    pub(super) fn merge_deferred_revivals(&self, changes: &mut Vec<LevelChange>) {
        let changed_positions = changes
            .iter()
            .map(|change| change.pos)
            .collect::<FxHashSet<_>>();
        let mut deferred = self.deferred_revivals.lock();
        for pos in &changed_positions {
            deferred.remove(pos);
        }
        changes.extend(deferred.drain().map(|(pos, revival)| LevelChange {
            pos,
            new_level: Some(revival.load_level),
            new_simulation_level: revival.simulation_level,
        }));
    }

    pub(super) fn prepare_ticking_readiness_demotions(
        &self,
        changes: &[LevelChange],
    ) -> Result<bool, FullNeighborhoodError> {
        if changes.is_empty() {
            return Ok(false);
        }

        let new_levels = changes
            .iter()
            .map(|change| (change.pos, change.new_level))
            .collect::<FxHashMap<_, _>>();
        let active_changes = changes
            .iter()
            .filter_map(|change| {
                self.chunks
                    .read_sync(&change.pos, |_, holder| Arc::clone(holder))
                    .map(|holder| (change.pos, holder, change.new_level))
            })
            .collect::<Vec<_>>();

        let dirty = {
            let mut neighborhood = self.full_neighborhood.lock();
            for (pos, holder, new_level) in &active_changes {
                if !new_level.is_some_and(is_full) {
                    neighborhood.remove_contributor_if_matches(*pos, holder)?;
                }
            }
            for change in changes {
                neighborhood.mark_dirty(change.pos);
            }
            neighborhood.dirty_counts_snapshot()
        };

        let candidates = self.readiness_candidates(&dirty, Some(&new_levels));
        let snapshot_changed = self.apply_readiness_demotions(&candidates);
        self.update_pending_readiness(&candidates);
        Ok(snapshot_changed)
    }

    #[cfg(test)]
    pub(super) fn reconcile_ticking_readiness(
        &self,
        changed_positions: &[ChunkPos],
    ) -> Result<bool, FullNeighborhoodError> {
        self.reconcile_ticking_readiness_measured(changed_positions)
            .map(|result| result.snapshot_changed)
    }

    pub(super) fn reconcile_ticking_readiness_measured(
        &self,
        changed_positions: &[ChunkPos],
    ) -> Result<ReadinessReconcileResult, FullNeighborhoodError> {
        let publications = self.full_publications.drain();
        if publications.is_empty() && changed_positions.is_empty() {
            return Ok(ReadinessReconcileResult::default());
        }
        let mut contributor_updates = FxHashMap::default();
        let mut activation_holders = FxHashMap::default();

        for &pos in changed_positions {
            contributor_updates.insert(pos, self.current_full_contributor(pos));
        }
        for publication in publications {
            if let Some(holder) = self.validate_full_publication(&publication) {
                contributor_updates.insert(publication.pos, Some(Arc::clone(&holder)));
                activation_holders.insert(publication.pos, holder);
            }
        }

        let dirty = {
            let mut neighborhood = self.full_neighborhood.lock();
            for &pos in changed_positions {
                neighborhood.mark_dirty(pos);
            }
            for (pos, holder) in &contributor_updates {
                neighborhood.reconcile_contributor(*pos, holder.as_ref())?;
            }
            neighborhood.take_dirty_counts()
        };

        let mut activation_holders = activation_holders.into_values().collect::<Vec<_>>();
        // Vanilla runs each Full-load callback from the main-executor task whose
        // ordering depends on asynchronous completion history. Steel deliberately
        // stabilizes this load-only cross-chunk tie by packed chunk position while
        // retaining each chunk's native block-entity map order.
        activation_holders.sort_unstable_by_key(|holder| PackedChunkPos::from(holder.get_pos()));
        self.activate_block_entities(&activation_holders);
        Ok(self.apply_final_readiness(dirty))
    }

    /// Rebuilds every gameplay ticking view from one optimized SCC traversal.
    ///
    /// This runs only after lifecycle/readiness changes, never as fixed per-tick
    /// bookkeeping. The published snapshot owns holders but never component guards,
    /// so callbacks cannot retain section or chunk-component locks.
    pub(crate) fn rebuild_ticking_chunk_snapshot(&self) -> usize {
        let mut block = Vec::new();
        let mut random_chunk_indices = Vec::new();
        let mut entity_indices = Vec::new();

        self.chunks.iter_sync(|pos, holder| {
            let Some(simulation_level) = holder.simulation_level() else {
                return true;
            };
            let readiness = holder.ticking_readiness_snapshot();
            if !simulation_level.is_block_ticking() || !readiness.is_block_ticking() {
                return true;
            }
            let Some(full) = holder.try_full_chunk() else {
                return true;
            };
            let randomly_ticking_sections =
                Arc::clone(full.common().sections.random_tick_sections());

            let index = block.len();
            block.push(TickableChunk {
                pos: *pos,
                holder: Arc::clone(holder),
                randomly_ticking_sections,
            });
            if simulation_level.is_entity_ticking() {
                random_chunk_indices.push(index);
                if readiness.is_entity_ticking() {
                    entity_indices.push(index);
                }
            }
            true
        });

        if let Err(error) = self
            .world_gen_context
            .world()
            .reconcile_active_scheduled_tick_chunks(block.iter().map(|chunk| chunk.pos))
        {
            panic!(
                "Full chunk scheduled-tick ownership invariant failed during ticking snapshot rebuild: {error:?}"
            );
        }

        let ticking_chunk_count = block.len();
        self.ticking_chunks.store(Arc::new(TickingChunkSnapshot {
            block: block.into_boxed_slice(),
            random_chunk_indices: random_chunk_indices.into_boxed_slice(),
            entity_indices: entity_indices.into_boxed_slice(),
        }));
        ticking_chunk_count
    }

    pub(super) fn ticking_snapshot_membership(
        readiness: TickingReadiness,
        simulation_level: Option<ChunkTicketLevel>,
    ) -> (bool, bool, bool) {
        let block = simulation_level.is_some_and(ChunkTicketLevel::is_block_ticking)
            && readiness != TickingReadiness::Unready;
        let random = block && simulation_level.is_some_and(ChunkTicketLevel::is_entity_ticking);
        let entity = random && readiness == TickingReadiness::EntityTicking;
        (block, random, entity)
    }

    pub(super) fn simulation_changes_ticking_snapshot(&self, changes: &[LevelChange]) -> bool {
        changes.iter().any(|change| {
            self.chunks
                .read_sync(&change.pos, |_, holder| {
                    let readiness = holder.ticking_readiness_snapshot().readiness();
                    Self::ticking_snapshot_membership(readiness, holder.simulation_level())
                        != Self::ticking_snapshot_membership(readiness, change.new_simulation_level)
                })
                .unwrap_or(false)
        })
    }

    // Readiness bookkeeping scans candidates once. Keep these lookups uncached so the
    // four-entry gameplay cache remains focused on repeated callback locality.
    pub(super) fn current_full_contributor(&self, pos: ChunkPos) -> Option<Arc<ChunkHolder>> {
        let holder = self
            .chunks
            .read_sync(&pos, |_, holder| Arc::clone(holder))?;
        if !holder.load_level().is_some_and(is_full)
            || !holder.is_full_status_initialized()
            || holder.published_status() != Some(ChunkStatus::Full)
            || holder.try_chunk(ChunkStatus::Full).is_none()
        {
            return None;
        }
        Some(holder)
    }

    pub(super) fn validate_full_publication(
        &self,
        publication: &FullPublication,
    ) -> Option<Arc<ChunkHolder>> {
        let published_holder = publication.holder.upgrade()?;
        let active_holder = self
            .chunks
            .read_sync(&publication.pos, |_, holder| Arc::clone(holder))?;
        if !Arc::ptr_eq(&published_holder, &active_holder)
            || !active_holder.load_level().is_some_and(is_full)
            || !active_holder.is_full_status_initialized()
            || active_holder.published_status() != Some(ChunkStatus::Full)
            || active_holder.try_chunk(ChunkStatus::Full).is_none()
        {
            return None;
        }
        Some(active_holder)
    }

    pub(super) fn readiness_candidates(
        &self,
        dirty: &[(ChunkPos, FullNeighborhoodCounts)],
        new_levels: Option<&FxHashMap<ChunkPos, Option<ChunkTicketLevel>>>,
    ) -> Vec<TickingReadinessCandidate> {
        dirty
            .iter()
            .filter_map(|(pos, counts)| {
                let holder = self.chunks.read_sync(pos, |_, holder| Arc::clone(holder))?;
                let load_level = match new_levels.and_then(|levels| levels.get(pos)) {
                    Some(level) => *level,
                    None => holder.load_level(),
                };
                let desired = Self::desired_ticking_readiness(load_level);
                let target = Self::target_ticking_readiness(&holder, load_level, *counts);
                Some(TickingReadinessCandidate {
                    pos: *pos,
                    holder,
                    desired,
                    target,
                })
            })
            .collect()
    }

    const fn desired_ticking_readiness(level: Option<ChunkTicketLevel>) -> TickingReadiness {
        if is_entity_ticking(level) {
            TickingReadiness::EntityTicking
        } else if is_block_ticking(level) {
            TickingReadiness::BlockTicking
        } else {
            TickingReadiness::Unready
        }
    }

    pub(super) fn target_ticking_readiness(
        holder: &ChunkHolder,
        level: Option<ChunkTicketLevel>,
        counts: FullNeighborhoodCounts,
    ) -> TickingReadiness {
        if !holder.is_full_status_initialized()
            || holder.published_status() != Some(ChunkStatus::Full)
            || holder.try_chunk(ChunkStatus::Full).is_none()
        {
            return TickingReadiness::Unready;
        }
        if is_entity_ticking(level) && counts.entity_ticking_ready() {
            TickingReadiness::EntityTicking
        } else if is_block_ticking(level) && counts.block_ticking_ready() {
            TickingReadiness::BlockTicking
        } else {
            TickingReadiness::Unready
        }
    }

    pub(super) fn apply_readiness_demotions(
        &self,
        candidates: &[TickingReadinessCandidate],
    ) -> bool {
        let world = self.world_gen_context.world();
        let mut snapshot_changed = false;
        for candidate in candidates {
            let current = candidate.holder.ticking_readiness_snapshot().readiness();
            if current <= candidate.target {
                continue;
            }
            let Some(previous) = candidate
                .holder
                .transition_ticking_readiness(candidate.target)
            else {
                continue;
            };
            let simulation_level = candidate.holder.simulation_level();
            snapshot_changed |= Self::ticking_snapshot_membership(previous, simulation_level)
                != Self::ticking_snapshot_membership(candidate.target, simulation_level);
            world.update_entity_chunk_visibility(
                candidate.pos,
                candidate.holder.entity_visibility(),
            );
        }
        snapshot_changed
    }

    pub(super) fn apply_final_readiness(
        &self,
        dirty: Vec<(ChunkPos, FullNeighborhoodCounts)>,
    ) -> ReadinessReconcileResult {
        if dirty.is_empty() {
            return ReadinessReconcileResult::default();
        }

        let candidates = self.readiness_candidates(&dirty, None);
        let mut result = ReadinessReconcileResult {
            snapshot_changed: self.apply_readiness_demotions(&candidates),
            candidate_count: candidates.len(),
            ..ReadinessReconcileResult::default()
        };
        self.update_pending_readiness(&candidates);

        let world = self.world_gen_context.world();
        for candidate in &candidates {
            let current = candidate.holder.ticking_readiness_snapshot().readiness();
            if current >= candidate.target {
                continue;
            }

            if current == TickingReadiness::Unready {
                let start = Instant::now();
                let post_process_result = candidate.holder.post_process_generation();
                result.post_process_generation += start.elapsed();
                let post_process_position_count = match post_process_result {
                    Ok(count) => count,
                    Err(error) => {
                        Self::log_postprocessing_failure(candidate, error);
                        continue;
                    }
                };
                result.post_process_chunk_count += 1;
                result.post_process_position_count += post_process_position_count;
            }

            if current == TickingReadiness::Unready
                && let Err(error) = world.unpack_scheduled_ticks(candidate.pos)
            {
                panic!(
                    "Full chunk scheduled-tick ownership invariant failed during readiness activation: {error:?}"
                );
            }

            let Some(previous) = candidate
                .holder
                .transition_ticking_readiness(candidate.target)
            else {
                continue;
            };
            let simulation_level = candidate.holder.simulation_level();
            result.snapshot_changed |=
                Self::ticking_snapshot_membership(previous, simulation_level)
                    != Self::ticking_snapshot_membership(candidate.target, simulation_level);
            world.update_entity_chunk_visibility(
                candidate.pos,
                candidate.holder.entity_visibility(),
            );
        }

        self.update_pending_readiness(&candidates);
        result
    }

    pub(super) fn update_pending_readiness(&self, candidates: &[TickingReadinessCandidate]) {
        let mut neighborhood = self.full_neighborhood.lock();
        for candidate in candidates {
            let confirmed = candidate.holder.ticking_readiness_snapshot().readiness();
            if confirmed < candidate.desired {
                neighborhood.ensure_pending_readiness(candidate.pos, &candidate.holder);
            } else {
                neighborhood.clear_pending_readiness(candidate.pos);
            }
        }
    }

    pub(super) fn log_postprocessing_failure(
        candidate: &TickingReadinessCandidate,
        error: PostProcessGenerationError,
    ) {
        tracing::error!(
            chunk = ?candidate.pos,
            ?error,
            desired = ?candidate.desired,
            target = ?candidate.target,
            load_level = ?candidate.holder.load_level(),
            "Failed to prepare Full chunk for ticking readiness"
        );
    }

    pub(super) fn clear_all_ticking_readiness(&self) {
        let world = self.world_gen_context.world();
        self.chunks.iter_sync(|pos, holder| {
            if holder
                .transition_ticking_readiness(TickingReadiness::Unready)
                .is_some()
            {
                world.update_entity_chunk_visibility(*pos, holder.entity_visibility());
            }
            true
        });
    }

    pub(super) fn rebuild_ticking_readiness(
        &self,
    ) -> Result<ReadinessReconcileResult, FullNeighborhoodError> {
        self.full_publications.drain();
        let mut active = Vec::new();
        self.chunks.iter_sync(|pos, holder| {
            active.push((*pos, Arc::clone(holder)));
            true
        });

        let mut rebuilt = FullNeighborhoodIndex::default();
        for (pos, holder) in &active {
            rebuilt.mark_dirty(*pos);
            let contributor = if holder.load_level().is_some_and(is_full)
                && holder.is_full_status_initialized()
                && holder.published_status() == Some(ChunkStatus::Full)
                && holder.try_chunk(ChunkStatus::Full).is_some()
            {
                Some(holder)
            } else {
                None
            };
            rebuilt.reconcile_contributor(*pos, contributor)?;
        }
        let dirty = rebuilt.take_dirty_counts();
        *self.full_neighborhood.lock() = rebuilt;
        let mut activation_holders = active.iter().map(|(_, holder)| holder).collect::<Vec<_>>();
        activation_holders.sort_unstable_by_key(|holder| PackedChunkPos::from(holder.get_pos()));
        self.activate_block_entities(activation_holders);
        Ok(self.apply_final_readiness(dirty))
    }

    pub(super) fn recover_ticking_readiness_index(
        &self,
        error: FullNeighborhoodError,
    ) -> ReadinessReconcileResult {
        tracing::error!(
            ?error,
            "Full-neighborhood index invariant failed; rebuilding from active chunks"
        );
        self.clear_all_ticking_readiness();
        *self.full_neighborhood.lock() = FullNeighborhoodIndex::default();
        match self.rebuild_ticking_readiness() {
            Ok(result) => result,
            Err(rebuild_error) => {
                tracing::error!(
                    ?rebuild_error,
                    "Failed to rebuild Full-neighborhood index; ticking readiness remains revoked"
                );
                self.clear_all_ticking_readiness();
                *self.full_neighborhood.lock() = FullNeighborhoodIndex::default();
                ReadinessReconcileResult::default()
            }
        }
    }
}
