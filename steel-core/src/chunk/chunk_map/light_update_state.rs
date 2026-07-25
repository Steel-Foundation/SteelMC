use super::{
    BlockPos, ChunkPos, FxHashMap, FxHashSet, LIGHT_CACHE_RADIUS, LightSectionEmptinessChange,
    Notify, SectionPos, SyncMutex, mem,
};

#[derive(Debug, Default)]
pub(super) struct PendingLightUpdates {
    pub(super) chunks: FxHashMap<ChunkPos, PendingChunkLightUpdates>,
    pub(super) queued_chunks: Vec<ChunkPos>,
}

impl PendingLightUpdates {
    pub(super) fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    pub(super) fn next_center(&self) -> Option<ChunkPos> {
        self.queued_chunks
            .iter()
            .copied()
            .find(|chunk_pos| self.chunks.contains_key(chunk_pos))
    }

    pub(super) fn next_center_touching_chunk(&self, chunk_pos: ChunkPos) -> Option<ChunkPos> {
        self.queued_chunks.iter().copied().find(|center| {
            self.chunks.contains_key(center) && light_update_window_contains(*center, chunk_pos)
        })
    }

    pub(super) fn queue_change(
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

    pub(super) fn drain(&mut self) -> Vec<(ChunkPos, PendingChunkLightUpdates)> {
        let mut chunks = mem::take(&mut self.chunks);
        let queued_chunks = mem::take(&mut self.queued_chunks);
        queued_chunks
            .into_iter()
            .filter_map(|chunk_pos| chunks.remove(&chunk_pos).map(|task| (chunk_pos, task)))
            .collect()
    }

    pub(super) fn drain_center(&mut self, chunk_pos: ChunkPos) -> Option<PendingChunkLightUpdates> {
        let task = self.chunks.remove(&chunk_pos)?;
        self.queued_chunks.retain(|&queued| queued != chunk_pos);
        Some(task)
    }

    pub(super) fn prepend_drained(&mut self, tasks: Vec<(ChunkPos, PendingChunkLightUpdates)>) {
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
pub(super) struct LightUpdateState {
    pub(super) pending: PendingLightUpdates,
    pub(super) in_flight_centers: FxHashMap<ChunkPos, usize>,
}

impl LightUpdateState {
    #[cfg(test)]
    pub(super) fn is_idle(&self) -> bool {
        self.pending.is_empty() && self.in_flight_centers.is_empty()
    }

    pub(super) fn has_in_flight_updates(&self) -> bool {
        !self.in_flight_centers.is_empty()
    }

    pub(super) fn has_in_flight_update_touching_chunk(&self, chunk_pos: ChunkPos) -> bool {
        self.in_flight_centers
            .keys()
            .copied()
            .any(|center| light_update_window_contains(center, chunk_pos))
    }

    pub(super) fn track_in_flight(&mut self, centers: &[ChunkPos]) {
        for &center in centers {
            *self.in_flight_centers.entry(center).or_default() += 1;
        }
    }

    pub(super) fn finish_in_flight(&mut self, centers: &[ChunkPos]) {
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

    pub(super) fn touches_chunk(&self, chunk_pos: ChunkPos) -> bool {
        self.pending
            .chunks
            .keys()
            .copied()
            .chain(self.in_flight_centers.keys().copied())
            .any(|center| light_update_window_contains(center, chunk_pos))
    }
}

pub(super) struct InFlightLightUpdates<'a> {
    pub(super) centers: Vec<ChunkPos>,
    pub(super) light_updates: &'a SyncMutex<LightUpdateState>,
    pub(super) progress_notify: &'a Notify,
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

pub(super) const fn light_update_window_contains(center: ChunkPos, chunk_pos: ChunkPos) -> bool {
    let dx = center.0.x.abs_diff(chunk_pos.0.x);
    let dz = center.0.y.abs_diff(chunk_pos.0.y);
    dx <= LIGHT_CACHE_RADIUS as u32 && dz <= LIGHT_CACHE_RADIUS as u32
}

#[derive(Debug, Default)]
pub(super) struct PendingChunkLightUpdates {
    pub(super) changed_positions: FxHashSet<BlockPos>,
    pub(super) changed_sections: FxHashMap<SectionPos, bool>,
}

impl PendingChunkLightUpdates {
    pub(super) fn is_empty(&self) -> bool {
        self.changed_positions.is_empty() && self.changed_sections.is_empty()
    }

    pub(super) fn merge_older(&mut self, older: Self) {
        self.changed_positions.extend(older.changed_positions);
        for (section_pos, empty) in older.changed_sections {
            self.changed_sections.entry(section_pos).or_insert(empty);
        }
    }

    pub(super) fn empty_section_changes(&self) -> Vec<LightSectionEmptinessChange> {
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
