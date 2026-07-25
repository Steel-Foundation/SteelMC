use super::{
    Arc, ChunkHolder, ChunkMap, ChunkPos, ChunkSaveDependency, ChunkStatus, ChunkStorage,
    ClearedBlockEntities, FinalizedBlockEntityUnload, FxHashSet, instrument, io, mem,
};

impl ChunkMap {
    /// Saves a chunk to disk. Does not remove from `unloading_chunks`.
    #[instrument(level = "trace", skip(self, chunk_holder, _save_dependency), fields(chunk = ?chunk_holder.get_pos()))]
    pub(super) async fn save_chunk(
        &self,
        chunk_holder: &Arc<ChunkHolder>,
        _save_dependency: ChunkSaveDependency,
    ) {
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

    pub(super) fn mark_chunk_dirty_for_save_retry(chunk_holder: &ChunkHolder) {
        let Some(chunk) = chunk_holder.try_chunk(ChunkStatus::StructureStarts) else {
            return;
        };
        chunk.mark_dirty();
    }

    /// Processes chunks that are pending unload.
    ///
    /// Iterates over `unloading_chunks`. For each chunk with `strong_count == 1`:
    /// - If staged to revive at the next lifecycle boundary: keep
    /// - If dirty: spawn save task (keep until saved and clean)
    /// - If not dirty: release region handle and remove
    #[instrument(level = "trace", skip(self, staged_revivals))]
    pub(super) fn process_unloads(self: &Arc<Self>, staged_revivals: &FxHashSet<ChunkPos>) {
        self.propagate_queued_light_changes();

        let mut finalized = Vec::new();
        {
            let light_updates = self.light_updates.lock();
            self.unloading_chunks.retain_sync(|pos, holder| {
                // Prepared ticket changes publish only at the next lifecycle boundary.
                if staged_revivals.contains(pos) {
                    return true;
                }

                if light_updates.touches_chunk(*pos) {
                    return true;
                }

                if Arc::strong_count(holder) != 1 {
                    return true;
                }

                let is_dirty = holder
                    .try_chunk(ChunkStatus::StructureStarts)
                    .is_some_and(|chunk| chunk.is_dirty());
                let has_save_pending_entities = self
                    .world_gen_context
                    .world()
                    .entity_manager()
                    .has_save_pending_for_chunk(*pos);

                if is_dirty || has_save_pending_entities {
                    let save_dependency = holder.add_save_dependency();
                    let holder_clone = Arc::clone(holder);
                    let map_clone = Arc::clone(self);
                    self.task_tracker.spawn(async move {
                        map_clone.save_chunk(&holder_clone, save_dependency).await;
                    });
                    return true;
                }

                let has_chunk = holder.try_chunk(ChunkStatus::Empty).is_some();
                finalized.push((*pos, Arc::clone(holder), has_chunk));
                false
            });
        }

        let world = self.world_gen_context.world();
        for (pos, holder, has_chunk) in finalized {
            let cleared = if has_chunk {
                holder
                    .try_chunk(ChunkStatus::Empty)
                    .map_or_else(ClearedBlockEntities::default, |chunk| {
                        chunk.clear_all_block_entities_staged()
                    })
            } else {
                ClearedBlockEntities::default()
            };
            self.finalized_block_entity_unloads
                .lock()
                .push(FinalizedBlockEntityUnload {
                    holder: Arc::clone(&holder),
                    lifecycle_dispatchers: cleared.lifecycle_dispatchers,
                    positions: cleared.positions,
                });

            world.unregister_full_chunk_ticks(pos);
            world.on_entity_chunk_unload_finalized(pos);
            if has_chunk {
                let map_clone = Arc::clone(self);
                self.task_tracker.spawn(async move {
                    if let Err(e) = map_clone.storage.release_chunk(pos).await {
                        tracing::error!(?pos, "Error releasing chunk: {e}");
                    }
                });
            }
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
            let _save_dependency = holder.add_save_dependency();
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
