use super::{
    Arc, BlockPos, ChunkHolder, ChunkMap, ChunkPos, ChunkStatus, InFlightLightUpdates,
    LightCacheLayout, LightCacheSetupRadius, LightLayer, LightSectionEmptinessChange,
    LightSectionRange, LightUpdateState, LightWorkset, PendingChunkLightUpdates, SectionPos,
    propagate_block_light_changes_with_empty_sections,
    propagate_sky_light_changes_with_empty_sections,
};

impl ChunkMap {
    /// Records a block change at the given position.
    /// This marks the chunk as having pending changes to broadcast.
    pub fn block_changed(&self, pos: BlockPos) {
        let chunk_pos = ChunkPos::new(
            SectionPos::block_to_section_coord(pos.0.x),
            SectionPos::block_to_section_coord(pos.0.z),
        );

        if let Some(holder) = self.lookup_active_holder(chunk_pos)
            && holder.block_changed(pos)
        {
            // First change for this chunk - add to broadcast list
            self.chunks_to_broadcast.lock().push(holder);
        }
    }

    /// Marks client-visible chunk packet content as changed.
    pub fn packet_content_changed(&self, chunk_pos: ChunkPos) {
        if let Some(holder) = self.lookup_active_holder(chunk_pos) {
            holder.mark_packet_content_changed();
        }
    }

    /// Records a light-section change at the given position.
    pub fn light_changed(&self, layer: LightLayer, section_pos: SectionPos) {
        let chunk_pos = ChunkPos::new(section_pos.x(), section_pos.z());

        if let Some(holder) = self.lookup_active_holder(chunk_pos) {
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

    pub(super) async fn flush_queued_light_changes_for_save(&self) {
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

    pub(super) fn drain_pending_light_updates(
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

    pub(super) fn next_pending_light_update_center(&self) -> Option<ChunkPos> {
        self.light_updates.lock().pending.next_center()
    }

    pub(super) fn next_pending_light_update_center_touching_chunk(
        &self,
        chunk_pos: ChunkPos,
    ) -> Option<ChunkPos> {
        self.light_updates
            .lock()
            .pending
            .next_center_touching_chunk(chunk_pos)
    }

    pub(super) fn drain_pending_light_update_for_center(
        &self,
        center: ChunkPos,
    ) -> Option<(PendingChunkLightUpdates, InFlightLightUpdates<'_>)> {
        let mut light_updates = self.light_updates.lock();
        let task = light_updates.pending.drain_center(center)?;
        let in_flight = self.track_in_flight_light_updates(&mut light_updates, vec![center]);
        Some((task, in_flight))
    }

    pub(super) fn track_in_flight_light_updates(
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

    pub(super) fn has_in_flight_light_updates(&self) -> bool {
        self.light_updates.lock().has_in_flight_updates()
    }

    pub(super) fn has_in_flight_light_update_touching_chunk(&self, chunk_pos: ChunkPos) -> bool {
        self.light_updates
            .lock()
            .has_in_flight_update_touching_chunk(chunk_pos)
    }

    pub(super) async fn wait_for_in_flight_light_updates(&self) {
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

    pub(super) async fn wait_for_in_flight_light_update_touching_chunk(&self, chunk_pos: ChunkPos) {
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

    pub(super) async fn flush_queued_light_changes_touching_chunk_for_save(
        &self,
        chunk_pos: ChunkPos,
    ) {
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
    pub(super) fn has_pending_light_updates(&self) -> bool {
        !self.light_updates.lock().is_idle()
    }

    #[cfg(test)]
    pub(super) fn light_update_touches_chunk(&self, chunk_pos: ChunkPos) -> bool {
        self.light_updates.lock().touches_chunk(chunk_pos)
    }

    pub(super) fn light_update_center_is_available(&self, center: ChunkPos) -> bool {
        self.light_update_holder(center)
            .is_some_and(|holder| holder.try_chunk(ChunkStatus::Light).is_some())
    }

    pub(super) fn light_update_holder(&self, chunk_pos: ChunkPos) -> Option<Arc<ChunkHolder>> {
        self.chunks
            .read_sync(&chunk_pos, |_, holder| Arc::clone(holder))
            .or_else(|| {
                self.unloading_chunks
                    .read_sync(&chunk_pos, |_, holder| Arc::clone(holder))
            })
    }

    pub(super) fn propagate_queued_light_change(
        &self,
        center: ChunkPos,
        task: PendingChunkLightUpdates,
    ) {
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

    pub(super) fn light_workset_for_change(&self, center: ChunkPos) -> Option<LightWorkset> {
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
}
