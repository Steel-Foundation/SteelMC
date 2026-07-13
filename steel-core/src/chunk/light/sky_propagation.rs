use steel_registry::{blocks::block_state_ext::BlockStateExt, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, Direction, SectionPos};

use super::{
    CachedLightBlock, LIGHT_BLOCKED, LightAxisDirection, LightCacheLayout, LightDirectionSet,
    LightLayer, LightLayerEdit, LightQueueFlags, LightSectionEmptinessChange,
    LightSectionReadCache, LightWorkset, MAX_LIGHT_LEVEL, PackedLightPropagationQueues,
    PackedLightQueueEntry, get_light_block_into, get_light_opacity, light_occlusion_shape,
};

/// Error returned when a sky-light propagation context is built from mismatched caches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkyLightPropagationContextError {
    /// Sky-light propagation requires a sky light edit cache.
    WrongLayer {
        /// Layer supplied by the edit cache.
        layer: LightLayer,
    },
    /// Section and light caches were built from different cache layouts.
    LayoutMismatch {
        /// Layout used by the section cache.
        section_layout: Box<LightCacheLayout>,
        /// Layout used by the light cache.
        light_layout: Box<LightCacheLayout>,
    },
    /// The workset does not contain its center chunk.
    MissingCenterChunk {
        /// Missing center chunk position.
        chunk_pos: ChunkPos,
    },
}

impl SkyLightPropagationContextError {
    fn layout_mismatch(section_layout: LightCacheLayout, light_layout: LightCacheLayout) -> Self {
        Self::LayoutMismatch {
            section_layout: Box::new(section_layout),
            light_layout: Box::new(light_layout),
        }
    }
}

/// Sections whose visible sky-light data changed during a scoped update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkyLightUpdateResult {
    /// Light sections that should be reported to the world/chunk update layer.
    pub updated_sections: Vec<SectionPos>,
}

/// Whether chunk sky-light generation must validate edge consistency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkyLightChunkEdgeChecks {
    /// Seed skylight and validate this chunk's horizontal edges against neighbors.
    Required,
    /// Trust existing neighboring light and pull initialized edge levels inward.
    Skipped,
}

/// Seeds and propagates sky light for the center chunk without edge checks.
pub fn propagate_sky_light_chunk_without_edge_checks(
    workset: &LightWorkset,
) -> Result<SkyLightUpdateResult, SkyLightPropagationContextError> {
    propagate_sky_light_chunk(workset, SkyLightChunkEdgeChecks::Skipped)
}

/// Seeds and propagates sky light for the center chunk of a scoped workset.
///
/// This matches `ScalableLux` `SkyStarLightEngine.lightChunk`: sky sections
/// around non-empty sections are initialized, full skylight is propagated
/// downward, then the caller chooses between validating edge consistency or
/// pulling already-initialized neighbor levels inward.
pub fn propagate_sky_light_chunk(
    workset: &LightWorkset,
    edge_checks: SkyLightChunkEdgeChecks,
) -> Result<SkyLightUpdateResult, SkyLightPropagationContextError> {
    workset.with_chunk_read_cache(|chunk_cache| {
        let layout = chunk_cache.layout();
        let Some(center_slot) = layout.cached_chunk(layout.center_chunk()) else {
            return Err(SkyLightPropagationContextError::MissingCenterChunk {
                chunk_pos: layout.center_chunk(),
            });
        };
        if chunk_cache.chunk(center_slot).is_none() {
            return Err(SkyLightPropagationContextError::MissingCenterChunk {
                chunk_pos: layout.center_chunk(),
            });
        }

        chunk_cache.with_section_read_cache(|section_cache| {
            chunk_cache.with_light_edit(LightLayer::Sky, |mut light_edit| {
                let mut queues = PackedLightPropagationQueues::new();

                {
                    let mut context = SkyLightPropagationContext::new(
                        section_cache,
                        &mut light_edit,
                        &mut queues,
                    )?;
                    context.reset_center_chunk_sections();
                    context.handle_unlit_empty_section_changes(layout.center_chunk());
                    context.light_chunk(layout.center_chunk(), edge_checks);
                    if edge_checks == SkyLightChunkEdgeChecks::Required {
                        context.deinit_and_lazy_init_empty_sections(layout.center_chunk(), true);
                    }
                }

                let mut updated_sections = Vec::new();
                light_edit.commit(None, |section_pos| updated_sections.push(section_pos));
                Ok(SkyLightUpdateResult { updated_sections })
            })
        })
    })
}

/// Force-synchronizes sky-light sections for an already-lit loaded chunk.
pub fn force_load_sky_light_chunk(
    workset: &LightWorkset,
) -> Result<SkyLightUpdateResult, SkyLightPropagationContextError> {
    workset.with_chunk_read_cache(|chunk_cache| {
        let layout = ensure_center_chunk(chunk_cache)?;

        chunk_cache.with_section_read_cache(|section_cache| {
            chunk_cache.with_light_edit(LightLayer::Sky, |mut light_edit| {
                let mut queues = PackedLightPropagationQueues::new();

                {
                    let mut context = SkyLightPropagationContext::new(
                        section_cache,
                        &mut light_edit,
                        &mut queues,
                    )?;
                    context.handle_loaded_empty_section_changes(layout.center_chunk());
                }

                let mut updated_sections = Vec::new();
                light_edit.commit(None, |section_pos| updated_sections.push(section_pos));
                Ok(SkyLightUpdateResult { updated_sections })
            })
        })
    })
}

/// Validates already-loaded sky-light chunk edges without resetting sections.
pub fn check_sky_light_chunk_edges(
    workset: &LightWorkset,
) -> Result<SkyLightUpdateResult, SkyLightPropagationContextError> {
    workset.with_chunk_read_cache(|chunk_cache| {
        let layout = ensure_center_chunk(chunk_cache)?;

        chunk_cache.with_section_read_cache(|section_cache| {
            chunk_cache.with_light_edit(LightLayer::Sky, |mut light_edit| {
                let mut queues = PackedLightPropagationQueues::new();

                {
                    let mut context = SkyLightPropagationContext::new(
                        section_cache,
                        &mut light_edit,
                        &mut queues,
                    )?;
                    context.light.rewrite_missing_sections_for_skylight();
                    for section_y in (layout.range().min_section_y()
                        ..layout.range().max_section_y_exclusive())
                        .rev()
                    {
                        context.check_missing_section(layout.center_chunk(), section_y, true);
                    }
                    context.check_chunk_edges(
                        layout.center_chunk(),
                        layout.range().min_section_y(),
                        layout.range().max_section_y_exclusive() - 1,
                    );
                }

                let mut updated_sections = Vec::new();
                light_edit.commit(None, |section_pos| updated_sections.push(section_pos));
                Ok(SkyLightUpdateResult { updated_sections })
            })
        })
    })
}

/// Loads already-persisted sky light and validates chunk edges without resetting sections.
pub fn load_sky_light_chunk(
    workset: &LightWorkset,
) -> Result<SkyLightUpdateResult, SkyLightPropagationContextError> {
    let mut updated_sections = force_load_sky_light_chunk(workset)?.updated_sections;
    updated_sections.extend(check_sky_light_chunk_edges(workset)?.updated_sections);
    Ok(SkyLightUpdateResult { updated_sections })
}

fn ensure_center_chunk(
    chunk_cache: &super::LightChunkReadCache<'_>,
) -> Result<LightCacheLayout, SkyLightPropagationContextError> {
    let layout = chunk_cache.layout();
    let Some(center_slot) = layout.cached_chunk(layout.center_chunk()) else {
        return Err(SkyLightPropagationContextError::MissingCenterChunk {
            chunk_pos: layout.center_chunk(),
        });
    };
    if chunk_cache.chunk(center_slot).is_none() {
        return Err(SkyLightPropagationContextError::MissingCenterChunk {
            chunk_pos: layout.center_chunk(),
        });
    }

    Ok(layout)
}

/// Runs ScalableLux-style sky-light propagation for changed blocks in a scoped workset.
pub fn propagate_sky_light_changes(
    workset: &LightWorkset,
    positions: impl IntoIterator<Item = BlockPos>,
) -> Result<SkyLightUpdateResult, SkyLightPropagationContextError> {
    propagate_sky_light_changes_with_empty_sections(workset, positions, [])
}

/// Runs sky-light propagation after applying real section emptiness transitions.
pub fn propagate_sky_light_changes_with_empty_sections(
    workset: &LightWorkset,
    positions: impl IntoIterator<Item = BlockPos>,
    empty_sections: impl IntoIterator<Item = LightSectionEmptinessChange>,
) -> Result<SkyLightUpdateResult, SkyLightPropagationContextError> {
    let positions = positions.into_iter().collect::<Vec<_>>();
    let empty_sections = empty_sections.into_iter().collect::<Vec<_>>();

    workset.with_chunk_read_cache(|chunk_cache| {
        let layout = chunk_cache.layout();
        // ScalableLux drops queued dynamic changes once the center chunk leaves the light cache.
        let Some(center_slot) = layout.cached_chunk(layout.center_chunk()) else {
            return Ok(SkyLightUpdateResult {
                updated_sections: Vec::new(),
            });
        };
        if chunk_cache.chunk(center_slot).is_none() {
            return Ok(SkyLightUpdateResult {
                updated_sections: Vec::new(),
            });
        }

        chunk_cache.with_section_read_cache(|section_cache| {
            chunk_cache.with_light_edit(LightLayer::Sky, |mut light_edit| {
                let mut queues = PackedLightPropagationQueues::new();

                {
                    let mut context = SkyLightPropagationContext::new(
                        section_cache,
                        &mut light_edit,
                        &mut queues,
                    )?;
                    let mut changed_chunks = Vec::new();
                    for change in &empty_sections {
                        let chunk_pos =
                            ChunkPos::new(change.section_pos.x(), change.section_pos.z());
                        context
                            .light
                            .set_section_empty(change.section_pos, change.empty);
                        if !changed_chunks.contains(&chunk_pos) {
                            changed_chunks.push(chunk_pos);
                        }
                    }
                    for chunk_pos in changed_chunks {
                        context.deinit_and_lazy_init_empty_sections(chunk_pos, false);
                    }
                    context.propagate_block_changes(&positions);
                }

                let mut updated_sections = Vec::new();
                light_edit.commit(None, |section_pos| updated_sections.push(section_pos));
                Ok(SkyLightUpdateResult { updated_sections })
            })
        })
    })
}

/// ScalableLux-style sky-light propagation over scoped Steel light caches.
pub struct SkyLightPropagationContext<'a, 'sections, 'light> {
    layout: LightCacheLayout,
    sections: &'a LightSectionReadCache<'sections>,
    light: &'a mut LightLayerEdit<'light>,
    queues: &'a mut PackedLightPropagationQueues,
    missing_section_checked: Vec<bool>,
}

impl<'a, 'sections, 'light> SkyLightPropagationContext<'a, 'sections, 'light> {
    /// Creates a sky-light propagation context from matching scoped caches.
    pub fn new(
        sections: &'a LightSectionReadCache<'sections>,
        light: &'a mut LightLayerEdit<'light>,
        queues: &'a mut PackedLightPropagationQueues,
    ) -> Result<Self, SkyLightPropagationContextError> {
        if light.layer() != LightLayer::Sky {
            return Err(SkyLightPropagationContextError::WrongLayer {
                layer: light.layer(),
            });
        }

        if sections.layout() != light.layout() {
            return Err(SkyLightPropagationContextError::layout_mismatch(
                sections.layout(),
                light.layout(),
            ));
        }

        let layout = light.layout();
        let section_count = layout.range().section_count();

        Ok(Self {
            layout,
            sections,
            light,
            queues,
            missing_section_checked: vec![false; section_count],
        })
    }

    /// Initializes the sky sections required around non-empty center sections.
    pub fn handle_unlit_empty_section_changes(&mut self, chunk_pos: ChunkPos) {
        self.initialize_chunk_sections(chunk_pos, true);
        self.deinit_and_lazy_init_empty_sections(chunk_pos, true);
    }

    /// Synchronizes sky sections for an already-lit loaded chunk without resetting light data.
    pub fn handle_loaded_empty_section_changes(&mut self, chunk_pos: ChunkPos) {
        self.initialize_chunk_sections(chunk_pos, false);
        self.deinit_and_lazy_init_empty_sections(chunk_pos, false);
    }

    fn initialize_chunk_sections(&mut self, chunk_pos: ChunkPos, unlit: bool) {
        for section_y in (self.layout.range().min_chunk_section_y()
            ..self.layout.range().max_chunk_section_y_exclusive())
            .rev()
        {
            let section_pos = SectionPos::new(chunk_pos.0.x, section_y, chunk_pos.0.y);
            if !self.section_is_non_empty(section_pos) {
                continue;
            }

            for offset_z in -1..=1 {
                for offset_x in -1..=1 {
                    let extrude = (offset_x | offset_z) != 0 || !unlit;
                    for offset_y in (-1..=1).rev() {
                        self.init_light_section(
                            SectionPos::new(
                                chunk_pos.0.x + offset_x,
                                section_y + offset_y,
                                chunk_pos.0.y + offset_z,
                            ),
                            extrude,
                            false,
                        );
                    }
                }
            }
        }
    }

    fn deinit_and_lazy_init_empty_sections(&mut self, chunk_pos: ChunkPos, unlit: bool) {
        for offset_z in -1..=1 {
            for offset_x in -1..=1 {
                let target_chunk =
                    ChunkPos::new(chunk_pos.0.x + offset_x, chunk_pos.0.y + offset_z);

                for section_y in (self.layout.range().min_section_y()
                    ..self.layout.range().max_section_y_exclusive())
                    .rev()
                {
                    let section_pos =
                        SectionPos::new(target_chunk.0.x, section_y, target_chunk.0.y);
                    match self.section_neighborhood_all_empty_if_known(target_chunk, section_y) {
                        Some(true) => {
                            self.light.set_section_missing(section_pos);
                        }
                        Some(false) => {
                            self.init_light_section(
                                section_pos,
                                (offset_x | offset_z) != 0 || !unlit,
                                false,
                            );
                        }
                        None => {
                            if !self.section_neighborhood_all_empty(target_chunk, section_y) {
                                self.init_light_section(
                                    section_pos,
                                    (offset_x | offset_z) != 0 || !unlit,
                                    false,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn section_neighborhood_all_empty(&self, chunk_pos: ChunkPos, section_y: i32) -> bool {
        for offset_y in -1..=1 {
            let neighbor_y = section_y + offset_y;
            if neighbor_y < self.layout.range().min_chunk_section_y()
                || neighbor_y >= self.layout.range().max_chunk_section_y_exclusive()
            {
                continue;
            }

            for offset_z in -1..=1 {
                for offset_x in -1..=1 {
                    let section_pos = SectionPos::new(
                        chunk_pos.0.x + offset_x,
                        neighbor_y,
                        chunk_pos.0.y + offset_z,
                    );
                    if let Some(empty) = self.sections.section_empty(section_pos) {
                        if !empty {
                            return false;
                        }
                    } else if let Some(empty) = self.light.section_empty(section_pos) {
                        if !empty {
                            return false;
                        }
                    } else if self.sections.has_non_empty_section(section_pos) {
                        return false;
                    }
                }
            }
        }

        true
    }

    fn section_neighborhood_all_empty_if_known(
        &self,
        chunk_pos: ChunkPos,
        section_y: i32,
    ) -> Option<bool> {
        for offset_y in -1..=1 {
            let neighbor_y = section_y + offset_y;
            if neighbor_y < self.layout.range().min_chunk_section_y()
                || neighbor_y >= self.layout.range().max_chunk_section_y_exclusive()
            {
                continue;
            }

            for offset_z in -1..=1 {
                for offset_x in -1..=1 {
                    let section_pos = SectionPos::new(
                        chunk_pos.0.x + offset_x,
                        neighbor_y,
                        chunk_pos.0.y + offset_z,
                    );
                    let empty = self.sections.section_empty(section_pos)?;
                    if !empty {
                        return Some(false);
                    }
                }
            }
        }

        Some(true)
    }

    /// Resets the center chunk to `ScalableLux`'s fresh all-missing lighting state.
    pub fn reset_center_chunk_sections(&mut self) {
        self.light
            .reset_chunk_sections_to_missing(self.layout.center_chunk());
    }

    /// Runs sky chunk lighting with the selected `ScalableLux` edge-check mode.
    pub fn light_chunk(&mut self, chunk_pos: ChunkPos, edge_checks: SkyLightChunkEdgeChecks) {
        self.light.rewrite_missing_sections_for_skylight();
        self.missing_section_checked.fill(false);

        let min_section = self.layout.range().min_chunk_section_y();
        let mut highest_non_empty_section = self.layout.range().max_chunk_section_y_exclusive() - 1;

        loop {
            let section_pos =
                SectionPos::new(chunk_pos.0.x, highest_non_empty_section, chunk_pos.0.y);
            if highest_non_empty_section != min_section - 1
                && self.sections.has_non_empty_section(section_pos)
            {
                break;
            }

            self.check_missing_section(chunk_pos, highest_non_empty_section, false);
            self.propagate_full_empty_section_edges(chunk_pos, highest_non_empty_section);

            if highest_non_empty_section == min_section - 1 {
                highest_non_empty_section -= 1;
                break;
            }
            highest_non_empty_section -= 1;
        }

        if highest_non_empty_section >= min_section {
            self.propagate_sky_sources_from_top(chunk_pos, highest_non_empty_section);
        }

        match edge_checks {
            SkyLightChunkEdgeChecks::Required => {
                self.perform_light_increase();
                for section_y in
                    (self.layout.range().min_section_y()..=highest_non_empty_section).rev()
                {
                    self.check_missing_section(chunk_pos, section_y, false);
                }
                self.check_chunk_edges(
                    chunk_pos,
                    self.layout.range().min_section_y(),
                    highest_non_empty_section,
                );
            }
            SkyLightChunkEdgeChecks::Skipped => {
                for section_y in
                    (self.layout.range().min_section_y()..=highest_non_empty_section).rev()
                {
                    self.check_missing_section(chunk_pos, section_y, false);
                }
                self.propagate_neighbor_levels(
                    chunk_pos,
                    self.layout.range().min_section_y(),
                    highest_non_empty_section,
                );
                self.perform_light_increase();
            }
        }
    }

    /// Handles one sky-light opacity change, matching `ScalableLux` `checkBlock`.
    pub fn check_block(&mut self, block_pos: BlockPos) -> bool {
        let Some(cached_block) = self.layout.cached_block(block_pos) else {
            return false;
        };

        let current_level = self.light.get(cached_block);
        if current_level == MAX_LIGHT_LEVEL {
            self.enqueue_increase(
                block_pos,
                current_level,
                LightDirectionSet::all(),
                LightQueueFlags::EMPTY.with(LightQueueFlags::HAS_SIDED_TRANSPARENT_BLOCKS),
            );
        } else {
            self.light.set(cached_block, 0);
        }

        self.enqueue_decrease(
            block_pos,
            current_level,
            LightDirectionSet::all(),
            LightQueueFlags::EMPTY,
        );
        true
    }

    /// Handles sky-light source and opacity changes for blocks in the center chunk.
    pub fn propagate_block_changes(&mut self, positions: &[BlockPos]) {
        self.light.rewrite_missing_sections_for_skylight();
        self.missing_section_checked.fill(false);

        let chunk_pos = self.layout.center_chunk();
        self.initialize_changed_sections(chunk_pos, positions);

        let mut changed_column_max_y = [i32::MIN; 16 * 16];
        for position in positions {
            if SectionPos::block_to_section_coord(position.x()) != chunk_pos.0.x
                || SectionPos::block_to_section_coord(position.z()) != chunk_pos.0.y
            {
                continue;
            }

            let index = ((position.x() & 15) | ((position.z() & 15) << 4)) as usize;
            changed_column_max_y[index] = changed_column_max_y[index].max(position.y());
        }

        let mut delayed_increases = Vec::new();
        let mut delayed_decreases = Vec::new();
        for (index, max_y) in changed_column_max_y.into_iter().enumerate() {
            if max_y == i32::MIN {
                continue;
            }

            let x = (chunk_pos.0.x << 4) | (index as i32 & 15);
            let z = (chunk_pos.0.y << 4) | ((index as i32 >> 4) & 15);
            let max_propagation_y =
                self.try_propagate_skylight_delayed(x, max_y, z, true, &mut delayed_increases);
            self.remove_sky_sources_below(x, max_propagation_y, z, &mut delayed_decreases);
        }

        self.process_delayed_increases(&delayed_increases);
        self.process_delayed_decreases(&delayed_decreases);

        for position in positions {
            self.check_block(*position);
        }

        self.perform_light_decrease();
    }

    /// Calculates the sky-light value that should exist at `block_pos`.
    #[must_use]
    pub fn calculate_light_value(&self, block_pos: BlockPos, expect: u8) -> Option<u8> {
        if expect == MAX_LIGHT_LEVEL {
            return Some(expect);
        }

        let cached_block = self.layout.cached_block(block_pos)?;
        let center_state = self.sections.get_block_state(cached_block);
        let opacity = get_light_opacity(center_state);
        let mut level = 0;

        for axis_direction in LightAxisDirection::ALL {
            let neighbor_pos = Self::offset(block_pos, axis_direction);
            let Some(neighbor_block) = self.layout.cached_block(neighbor_pos) else {
                continue;
            };
            let neighbor_level = self.light.get(neighbor_block);
            if neighbor_level.saturating_sub(1) <= level {
                continue;
            }

            let neighbor_state = self.sections.get_block_state(neighbor_block);
            if get_light_block_into(
                neighbor_state,
                center_state,
                axis_direction.opposite().direction(),
                opacity,
            ) == LIGHT_BLOCKED
            {
                continue;
            }

            level = level.max(neighbor_level.saturating_sub(opacity));
            if level > expect {
                return Some(level);
            }
        }

        Some(level)
    }

    fn init_light_section(&mut self, section_pos: SectionPos, extrude: bool, init_removed: bool) {
        if self.layout.section_slot(section_pos).is_none()
            || (!self.light.has_cached_section(section_pos)
                && (!init_removed || !self.light.materialize_removed_missing_section(section_pos)))
        {
            return;
        }
        if !self.light.is_section_missing(section_pos) {
            return;
        }

        let mut highest_non_empty_section = self.layout.range().min_section_y() - 1;
        for section_y in (self.layout.range().min_chunk_section_y()
            ..self.layout.range().max_chunk_section_y_exclusive())
            .rev()
        {
            let candidate = SectionPos::new(section_pos.x(), section_y, section_pos.z());
            if self.section_is_non_empty(candidate) {
                highest_non_empty_section = section_y;
                break;
            }
        }

        if section_pos.y() > highest_non_empty_section {
            self.light.set_section_non_missing(section_pos);
            self.light.fill_section(section_pos, MAX_LIGHT_LEVEL);
        } else if extrude {
            self.light
                .extrude_lower_from_first_section_above(section_pos);
        } else {
            self.light.set_section_non_missing(section_pos);
        }
    }

    fn section_is_non_empty(&self, section_pos: SectionPos) -> bool {
        if let Some(empty) = self.sections.section_empty(section_pos) {
            return !empty;
        }

        if let Some(empty) = self.light.section_empty(section_pos) {
            return !empty;
        }

        self.sections.has_non_empty_section(section_pos)
    }

    fn check_missing_section(
        &mut self,
        chunk_pos: ChunkPos,
        section_y: i32,
        extrude_initialized: bool,
    ) -> bool {
        let Some(section_index) = self.layout.range().section_index(section_y) else {
            return false;
        };
        if self.missing_section_checked[section_index] {
            return false;
        }
        self.missing_section_checked[section_index] = true;

        let center_section_pos = SectionPos::new(chunk_pos.0.x, section_y, chunk_pos.0.y);
        let mut need_init_neighbors = self.light.has_non_missing_section(center_section_pos);
        if !need_init_neighbors {
            'neighbor_search: for offset_z in -1..=1 {
                for offset_x in -1..=1 {
                    let section_pos = SectionPos::new(
                        chunk_pos.0.x + offset_x,
                        section_y,
                        chunk_pos.0.y + offset_z,
                    );
                    if self.light.has_non_missing_section(section_pos) {
                        need_init_neighbors = true;
                        break 'neighbor_search;
                    }
                }
            }
        }

        if need_init_neighbors {
            for offset_z in -1..=1 {
                for offset_x in -1..=1 {
                    self.init_light_section(
                        SectionPos::new(
                            chunk_pos.0.x + offset_x,
                            section_y,
                            chunk_pos.0.y + offset_z,
                        ),
                        if (offset_x | offset_z) == 0 {
                            extrude_initialized
                        } else {
                            true
                        },
                        true,
                    );
                }
            }
        }

        need_init_neighbors
    }

    fn propagate_full_empty_section_edges(&mut self, chunk_pos: ChunkPos, section_y: i32) {
        for direction in LightAxisDirection::HORIZONTAL {
            let (neighbor_offset_x, _, neighbor_offset_z) = direction.offset();
            let neighbor_section_pos = SectionPos::new(
                chunk_pos.0.x + neighbor_offset_x,
                section_y,
                chunk_pos.0.y + neighbor_offset_z,
            );
            if !self.light.has_non_missing_section(neighbor_section_pos) {
                continue;
            }

            let (increment_x, increment_z, start_x, start_z) =
                Self::current_edge_scan(chunk_pos, direction);
            let directions = LightDirectionSet::only(direction);
            let min_y = section_y << 4;
            let max_y = min_y | 15;
            for y in min_y..=max_y {
                let mut x = start_x;
                let mut z = start_z;
                for _ in 0..16 {
                    self.enqueue_increase(
                        BlockPos::new(x, y, z),
                        MAX_LIGHT_LEVEL,
                        directions,
                        LightQueueFlags::EMPTY,
                    );
                    x += increment_x;
                    z += increment_z;
                }
            }
        }
    }

    fn propagate_sky_sources_from_top(&mut self, chunk_pos: ChunkPos, highest_section: i32) {
        let section_min_x = chunk_pos.0.x << 4;
        let section_min_z = chunk_pos.0.y << 4;
        let start_y = (highest_section << 4) | 15;

        for z in 0..super::CHUNK_EDGE {
            for x in 0..super::CHUNK_EDGE {
                self.try_propagate_skylight_inner(
                    section_min_x + x as i32,
                    start_y + 1,
                    section_min_z + z as i32,
                    false,
                    None,
                );
            }
        }
    }

    fn try_propagate_skylight_delayed(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        extrude_initialized: bool,
        delayed_increases: &mut Vec<PackedLightQueueEntry>,
    ) -> i32 {
        self.try_propagate_skylight_inner(x, y, z, extrude_initialized, Some(delayed_increases))
    }

    fn try_propagate_skylight_inner(
        &mut self,
        x: i32,
        mut y: i32,
        z: i32,
        extrude_initialized: bool,
        mut delayed_increases: Option<&mut Vec<PackedLightQueueEntry>>,
    ) -> i32 {
        if self.get_light_level_extruded(BlockPos::new(x, y + 1, z)) != MAX_LIGHT_LEVEL {
            return y;
        }

        self.check_missing_section(
            ChunkPos::new(
                SectionPos::block_to_section_coord(x),
                SectionPos::block_to_section_coord(z),
            ),
            SectionPos::block_to_section_coord(y),
            extrude_initialized,
        );

        let mut above_state = self.block_state(BlockPos::new(x, y + 1, z));
        while y >= (self.layout.range().min_section_y() << 4) {
            if (y & 15) == 15 {
                self.check_missing_section(
                    ChunkPos::new(
                        SectionPos::block_to_section_coord(x),
                        SectionPos::block_to_section_coord(z),
                    ),
                    SectionPos::block_to_section_coord(y),
                    extrude_initialized,
                );
            }

            let current_pos = BlockPos::new(x, y, z);
            let current_state = self.block_state(current_pos);
            let opacity = current_state.get_light_dampening();
            if get_light_block_into(above_state, current_state, Direction::Down, opacity)
                == LIGHT_BLOCKED
                || opacity > 0
            {
                break;
            }

            let section_pos = SectionPos::from_block_pos(current_pos);
            if self.light.has_non_missing_section(section_pos) {
                let Some(cached_block) = self.layout.cached_block(current_pos) else {
                    break;
                };
                let increase_entry = self.enqueue_increase(
                    current_pos,
                    MAX_LIGHT_LEVEL,
                    LightDirectionSet::all_except(LightAxisDirection::PositiveY),
                    Self::shape_flags(current_state),
                );
                above_state = current_state;

                if let Some(delayed_increases) = delayed_increases.as_deref_mut() {
                    if let Some(entry) = increase_entry {
                        delayed_increases.push(entry);
                    }
                } else {
                    self.light.set(cached_block, MAX_LIGHT_LEVEL);
                }
            } else {
                y &= !15;
                above_state = Self::air();
            }

            y -= 1;
        }

        y
    }

    fn initialize_changed_sections(&mut self, chunk_pos: ChunkPos, positions: &[BlockPos]) {
        let mut section_ys = Vec::new();
        for position in positions {
            if SectionPos::block_to_section_coord(position.x()) != chunk_pos.0.x
                || SectionPos::block_to_section_coord(position.z()) != chunk_pos.0.y
            {
                continue;
            }

            let section_y = SectionPos::block_to_section_coord(position.y());
            if !section_ys.contains(&section_y) {
                section_ys.push(section_y);
            }
        }

        for section_y in section_ys {
            let section_pos = SectionPos::new(chunk_pos.0.x, section_y, chunk_pos.0.y);
            if !self.sections.has_non_empty_section(section_pos) {
                continue;
            }

            for offset_z in -1..=1 {
                for offset_x in -1..=1 {
                    for offset_y in (-1..=1).rev() {
                        self.init_light_section(
                            SectionPos::new(
                                chunk_pos.0.x + offset_x,
                                section_y + offset_y,
                                chunk_pos.0.y + offset_z,
                            ),
                            true,
                            false,
                        );
                    }
                }
            }
        }
    }

    fn remove_sky_sources_below(
        &mut self,
        x: i32,
        mut y: i32,
        z: i32,
        delayed_decreases: &mut Vec<PackedLightQueueEntry>,
    ) {
        if self.get_light_level_extruded(BlockPos::new(x, y, z)) != MAX_LIGHT_LEVEL {
            return;
        }

        let min_y = self.layout.range().min_section_y() << 4;
        while y >= min_y {
            if (y & 15) == 15 {
                self.check_missing_section(
                    ChunkPos::new(
                        SectionPos::block_to_section_coord(x),
                        SectionPos::block_to_section_coord(z),
                    ),
                    SectionPos::block_to_section_coord(y),
                    true,
                );
            }

            let current_pos = BlockPos::new(x, y, z);
            let section_pos = SectionPos::from_block_pos(current_pos);
            if !self.light.has_non_missing_section(section_pos) {
                y &= !15;
                y -= 1;
                continue;
            }

            let Some(cached_block) = self.layout.cached_block(current_pos) else {
                break;
            };
            if self.light.get(cached_block) != MAX_LIGHT_LEVEL {
                break;
            }

            if let Some(entry) = self.enqueue_decrease(
                current_pos,
                MAX_LIGHT_LEVEL,
                LightDirectionSet::all_except(LightAxisDirection::PositiveY),
                LightQueueFlags::EMPTY,
            ) {
                delayed_decreases.push(entry);
            }
            y -= 1;
        }
    }

    fn process_delayed_increases(&mut self, entries: &[PackedLightQueueEntry]) {
        for entry in entries {
            let Some(source_block) = self.cached_block_from_entry(*entry) else {
                continue;
            };
            self.light.set(source_block, entry.level());
        }
    }

    fn process_delayed_decreases(&mut self, entries: &[PackedLightQueueEntry]) {
        for entry in entries {
            let Some(source_block) = self.cached_block_from_entry(*entry) else {
                continue;
            };
            self.light.set(source_block, 0);
        }
    }

    fn get_light_level_extruded(&self, block_pos: BlockPos) -> u8 {
        let mut section_y = SectionPos::block_to_section_coord(block_pos.y());
        let section_x = SectionPos::block_to_section_coord(block_pos.x());
        let section_z = SectionPos::block_to_section_coord(block_pos.z());

        if let Some(cached_block) = self.layout.cached_block(block_pos)
            && self
                .light
                .has_non_missing_section(SectionPos::new(section_x, section_y, section_z))
        {
            return self.light.get(cached_block);
        }

        loop {
            section_y += 1;
            if section_y >= self.layout.range().max_section_y_exclusive() {
                return MAX_LIGHT_LEVEL;
            }

            let section_pos = SectionPos::new(section_x, section_y, section_z);
            if !self.light.has_non_missing_section(section_pos) {
                continue;
            }
            let block_above = BlockPos::new(block_pos.x(), section_y << 4, block_pos.z());
            let Some(cached_block) = self.layout.cached_block(block_above) else {
                continue;
            };
            return self.light.get(cached_block);
        }
    }

    fn propagate_neighbor_levels(
        &mut self,
        chunk_pos: ChunkPos,
        from_section: i32,
        to_section: i32,
    ) {
        for section_y in (from_section..=to_section).rev() {
            let section_pos = SectionPos::new(chunk_pos.0.x, section_y, chunk_pos.0.y);
            if !self.light.has_non_missing_section(section_pos) {
                continue;
            }

            for direction in LightAxisDirection::HORIZONTAL {
                self.propagate_neighbor_level_section(chunk_pos, section_y, direction);
            }
        }
    }

    fn propagate_neighbor_level_section(
        &mut self,
        chunk_pos: ChunkPos,
        section_y: i32,
        direction: LightAxisDirection,
    ) {
        let (neighbor_offset_x, _, neighbor_offset_z) = direction.offset();
        let neighbor_section_pos = SectionPos::new(
            chunk_pos.0.x + neighbor_offset_x,
            section_y,
            chunk_pos.0.y + neighbor_offset_z,
        );
        if !self.light.has_light_data_section(neighbor_section_pos) {
            return;
        }

        let (increment_x, increment_z, start_x, start_z) =
            Self::neighbor_edge_scan(chunk_pos, direction);
        let directions = LightDirectionSet::only(direction.opposite());
        let flags = LightQueueFlags::EMPTY.with(LightQueueFlags::HAS_SIDED_TRANSPARENT_BLOCKS);

        let min_y = section_y << 4;
        let max_y = min_y | 15;
        for y in min_y..=max_y {
            let mut x = start_x;
            let mut z = start_z;
            for _ in 0..16 {
                let source_pos = BlockPos::new(x, y, z);
                let Some(source_block) = self.layout.cached_block(source_pos) else {
                    x += increment_x;
                    z += increment_z;
                    continue;
                };
                let level = self.light.get(source_block);
                if level > 1 {
                    self.enqueue_increase(source_pos, level, directions, flags);
                }
                x += increment_x;
                z += increment_z;
            }
        }
    }

    fn check_chunk_edges(&mut self, chunk_pos: ChunkPos, from_section: i32, to_section: i32) {
        for section_y in (from_section..=to_section).rev() {
            self.check_chunk_edge(chunk_pos, section_y);
        }

        self.perform_light_decrease();
    }

    fn check_chunk_edge(&mut self, chunk_pos: ChunkPos, section_y: i32) {
        let current_section_pos = SectionPos::new(chunk_pos.0.x, section_y, chunk_pos.0.y);
        if !self.light.has_non_missing_section(current_section_pos) {
            return;
        }

        for direction in LightAxisDirection::HORIZONTAL {
            let (neighbor_offset_x, _, neighbor_offset_z) = direction.offset();
            let neighbor_chunk_pos = ChunkPos::new(
                chunk_pos.0.x + neighbor_offset_x,
                chunk_pos.0.y + neighbor_offset_z,
            );
            let neighbor_section_pos =
                SectionPos::new(neighbor_chunk_pos.0.x, section_y, neighbor_chunk_pos.0.y);
            if !self.light.has_non_missing_section(neighbor_section_pos) {
                continue;
            }
            if !self.light.has_light_data_section(current_section_pos)
                && !self.light.has_light_data_section(neighbor_section_pos)
            {
                continue;
            }

            self.check_chunk_edge_direction(chunk_pos, neighbor_chunk_pos, section_y, direction);
        }
    }

    fn check_chunk_edge_direction(
        &mut self,
        chunk_pos: ChunkPos,
        neighbor_chunk_pos: ChunkPos,
        section_y: i32,
        direction: LightAxisDirection,
    ) {
        let (neighbor_offset_x, _, neighbor_offset_z) = direction.offset();
        let (increment_x, increment_z, start_x, start_z) =
            Self::current_edge_scan(chunk_pos, direction);
        let mut center_delayed_checks = [0usize; 16 * 16];
        let mut neighbor_delayed_checks = [0usize; 16 * 16];
        let mut center_delayed_check_count = 0;
        let mut neighbor_delayed_check_count = 0;

        let min_y = section_y << 4;
        let max_y = min_y | 15;
        for y in min_y..=max_y {
            let mut x = start_x;
            let mut z = start_z;
            for _ in 0..16 {
                let current_pos = BlockPos::new(x, y, z);
                let neighbor_pos = BlockPos::new(x + neighbor_offset_x, y, z + neighbor_offset_z);
                let Some(current_block) = self.layout.cached_block(current_pos) else {
                    x += increment_x;
                    z += increment_z;
                    continue;
                };
                let Some(neighbor_block) = self.layout.cached_block(neighbor_pos) else {
                    x += increment_x;
                    z += increment_z;
                    continue;
                };

                let current_level = self.light.get(current_block);
                if self
                    .calculate_light_value(current_pos, current_level)
                    .is_some_and(|calculated| calculated != current_level)
                {
                    center_delayed_checks[center_delayed_check_count] = current_block.local_index;
                    center_delayed_check_count += 1;
                }

                let neighbor_level = self.light.get(neighbor_block);
                if self
                    .calculate_light_value(neighbor_pos, neighbor_level)
                    .is_some_and(|calculated| calculated != neighbor_level)
                {
                    neighbor_delayed_checks[neighbor_delayed_check_count] =
                        neighbor_block.local_index;
                    neighbor_delayed_check_count += 1;
                }

                x += increment_x;
                z += increment_z;
            }
        }

        let current_chunk_offset_x = chunk_pos.0.x << 4;
        let current_chunk_offset_z = chunk_pos.0.y << 4;
        let neighbor_chunk_offset_x = neighbor_chunk_pos.0.x << 4;
        let neighbor_chunk_offset_z = neighbor_chunk_pos.0.y << 4;
        let chunk_offset_y = section_y << 4;
        let delayed_check_count = center_delayed_check_count.max(neighbor_delayed_check_count);
        for delayed_check_index in 0..delayed_check_count {
            if delayed_check_index < center_delayed_check_count {
                let local_index = center_delayed_checks[delayed_check_index];
                self.check_block(Self::block_pos_from_local_index(
                    current_chunk_offset_x,
                    chunk_offset_y,
                    current_chunk_offset_z,
                    local_index,
                ));
            }
            if delayed_check_index < neighbor_delayed_check_count {
                let local_index = neighbor_delayed_checks[delayed_check_index];
                self.check_block(Self::block_pos_from_local_index(
                    neighbor_chunk_offset_x,
                    chunk_offset_y,
                    neighbor_chunk_offset_z,
                    local_index,
                ));
            }
        }
    }

    fn perform_light_increase(&mut self) {
        while let Some(entry) = self.queues.dequeue_increase() {
            let Some(source_block) = self.cached_block_from_entry(entry) else {
                continue;
            };
            if entry.should_recheck_level() {
                if self.light.get(source_block) != entry.level() {
                    continue;
                }
            } else if entry.should_write_level() {
                self.light.set(source_block, entry.level());
            }

            let source_state = if entry.has_sided_transparent_blocks() {
                Some(self.sections.get_block_state(source_block))
            } else {
                None
            };

            for axis_direction in entry.directions().directions() {
                let neighbor_pos = Self::offset(source_block.block_pos, axis_direction);
                let Some(neighbor_block) = self.layout.cached_block(neighbor_pos) else {
                    continue;
                };
                if !self.light.has_non_missing(neighbor_block) {
                    continue;
                }
                let current_level = self.light.get(neighbor_block);
                if current_level >= entry.level().saturating_sub(1) {
                    continue;
                }

                let neighbor_state = self.sections.get_block_state(neighbor_block);
                let Some((target_level, flags)) = Self::target_level(
                    entry.level(),
                    source_state,
                    neighbor_state,
                    axis_direction.direction(),
                ) else {
                    continue;
                };
                if target_level <= current_level {
                    continue;
                }

                self.light.set(neighbor_block, target_level);
                if target_level > 1 {
                    self.enqueue_increase(
                        neighbor_pos,
                        target_level,
                        LightDirectionSet::all_except_opposite(axis_direction),
                        flags,
                    );
                }
            }
        }
    }

    fn perform_light_decrease(&mut self) {
        while let Some(entry) = self.queues.dequeue_decrease() {
            let Some(source_block) = self.cached_block_from_entry(entry) else {
                continue;
            };
            let source_state = if entry.has_sided_transparent_blocks() {
                Some(self.sections.get_block_state(source_block))
            } else {
                None
            };

            for axis_direction in entry.directions().directions() {
                let neighbor_pos = Self::offset(source_block.block_pos, axis_direction);
                let Some(neighbor_block) = self.layout.cached_block(neighbor_pos) else {
                    continue;
                };
                if !self.light.has_non_missing(neighbor_block) {
                    continue;
                }
                let current_level = self.light.get(neighbor_block);
                if current_level == 0 {
                    continue;
                }

                let neighbor_state = self.sections.get_block_state(neighbor_block);
                let Some((target_level, flags)) = Self::target_level_saturating(
                    entry.level(),
                    source_state,
                    neighbor_state,
                    axis_direction.direction(),
                ) else {
                    continue;
                };

                if current_level > target_level {
                    self.enqueue_increase(
                        neighbor_pos,
                        current_level,
                        LightDirectionSet::all(),
                        flags.with(LightQueueFlags::RECHECK_LEVEL),
                    );
                    continue;
                }

                self.light.set(neighbor_block, 0);
                if target_level > 0 {
                    self.enqueue_decrease(
                        neighbor_pos,
                        target_level,
                        LightDirectionSet::all_except_opposite(axis_direction),
                        flags,
                    );
                }
            }
        }

        self.perform_light_increase();
    }

    fn target_level(
        propagated_level: u8,
        source_state: Option<BlockStateId>,
        target_state: BlockStateId,
        direction: Direction,
    ) -> Option<(u8, LightQueueFlags)> {
        let source_state = match source_state {
            Some(source_state) => source_state,
            None => Self::air(),
        };
        let opacity = get_light_block_into(
            source_state,
            target_state,
            direction,
            get_light_opacity(target_state),
        );
        if opacity == LIGHT_BLOCKED || opacity >= propagated_level {
            return None;
        }

        Some((propagated_level - opacity, Self::shape_flags(target_state)))
    }

    fn target_level_saturating(
        propagated_level: u8,
        source_state: Option<BlockStateId>,
        target_state: BlockStateId,
        direction: Direction,
    ) -> Option<(u8, LightQueueFlags)> {
        let source_state = match source_state {
            Some(source_state) => source_state,
            None => Self::air(),
        };
        let opacity = get_light_block_into(
            source_state,
            target_state,
            direction,
            get_light_opacity(target_state),
        );
        if opacity == LIGHT_BLOCKED {
            return None;
        }

        Some((
            propagated_level.saturating_sub(opacity),
            Self::shape_flags(target_state),
        ))
    }

    fn cached_block_from_entry(&self, entry: PackedLightQueueEntry) -> Option<CachedLightBlock> {
        self.layout.cached_block_from_packed(entry.block_pos())
    }

    fn enqueue_decrease(
        &mut self,
        block_pos: BlockPos,
        level: u8,
        directions: LightDirectionSet,
        flags: LightQueueFlags,
    ) -> Option<PackedLightQueueEntry> {
        let packed_pos = self.layout.encode_block_pos(block_pos)?;
        let entry = PackedLightQueueEntry::from_parts(packed_pos, level, directions, flags);
        self.queues.enqueue_decrease(entry);
        Some(entry)
    }

    fn enqueue_increase(
        &mut self,
        block_pos: BlockPos,
        level: u8,
        directions: LightDirectionSet,
        flags: LightQueueFlags,
    ) -> Option<PackedLightQueueEntry> {
        let packed_pos = self.layout.encode_block_pos(block_pos)?;
        let entry = PackedLightQueueEntry::from_parts(packed_pos, level, directions, flags);
        self.queues.enqueue_increase(entry);
        Some(entry)
    }

    fn block_state(&self, block_pos: BlockPos) -> BlockStateId {
        let Some(cached_block) = self.layout.cached_block(block_pos) else {
            return Self::air();
        };
        self.sections.get_block_state(cached_block)
    }

    const fn current_edge_scan(
        chunk_pos: ChunkPos,
        direction: LightAxisDirection,
    ) -> (i32, i32, i32, i32) {
        let (offset_x, _, offset_z) = direction.offset();
        if offset_x != 0 {
            let start_x = if offset_x < 0 {
                chunk_pos.0.x << 4
            } else {
                (chunk_pos.0.x << 4) | 15
            };
            return (0, 1, start_x, chunk_pos.0.y << 4);
        }

        let start_z = if offset_z < 0 {
            chunk_pos.0.y << 4
        } else {
            (chunk_pos.0.y << 4) | 15
        };
        (1, 0, chunk_pos.0.x << 4, start_z)
    }

    const fn neighbor_edge_scan(
        chunk_pos: ChunkPos,
        direction: LightAxisDirection,
    ) -> (i32, i32, i32, i32) {
        let (offset_x, _, offset_z) = direction.offset();
        if offset_x != 0 {
            let start_x = if offset_x < 0 {
                (chunk_pos.0.x << 4) - 1
            } else {
                (chunk_pos.0.x << 4) + 16
            };
            return (0, 1, start_x, chunk_pos.0.y << 4);
        }

        let start_z = if offset_z < 0 {
            (chunk_pos.0.y << 4) - 1
        } else {
            (chunk_pos.0.y << 4) + 16
        };
        (1, 0, chunk_pos.0.x << 4, start_z)
    }

    const fn block_pos_from_local_index(
        chunk_offset_x: i32,
        chunk_offset_y: i32,
        chunk_offset_z: i32,
        local_index: usize,
    ) -> BlockPos {
        BlockPos::new(
            chunk_offset_x | (local_index & 15) as i32,
            chunk_offset_y | (local_index >> 8) as i32,
            chunk_offset_z | ((local_index >> 4) & 15) as i32,
        )
    }

    fn shape_flags(block_state: BlockStateId) -> LightQueueFlags {
        if light_occlusion_shape(block_state).is_empty() {
            LightQueueFlags::EMPTY
        } else {
            LightQueueFlags::EMPTY.with(LightQueueFlags::HAS_SIDED_TRANSPARENT_BLOCKS)
        }
    }

    const fn offset(block_pos: BlockPos, direction: LightAxisDirection) -> BlockPos {
        let (dx, dy, dz) = direction.offset();
        block_pos.offset(dx, dy, dz)
    }

    fn air() -> BlockStateId {
        vanilla_blocks::AIR.default_state()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use steel_registry::{test_support::init_test_registry, vanilla_blocks};
    use steel_utils::types::UpdateFlags;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::chunk::{
        chunk_access::{ChunkAccess, ChunkStatus},
        chunk_holder::ChunkHolder,
        chunk_ticket_manager::ChunkTicketLevel,
        light::{LightCacheSetupRadius, LightSection, LightSectionData, LightSectionRange},
        proto_chunk::ProtoChunk,
        section::{ChunkSection, Sections},
    };

    fn init_tests() {
        init_test_registry();
        init_behaviors();
    }

    fn range() -> LightSectionRange {
        let Ok(range) = LightSectionRange::from_world_height(0, 16) else {
            panic!("test height should create a valid light range");
        };
        range
    }

    fn holder_with_section(pos: ChunkPos, section: ChunkSection) -> Arc<ChunkHolder> {
        holder_with_sections(pos, vec![section])
    }

    fn holder_with_sections(pos: ChunkPos, sections: Vec<ChunkSection>) -> Arc<ChunkHolder> {
        let height = (sections.len() * 16) as i32;
        let proto = ProtoChunk::new(
            Sections::from_owned(sections.into_boxed_slice()),
            pos,
            0,
            height,
            Weak::new(),
        );
        proto.initialize_light_sources();
        let holder = Arc::new(ChunkHolder::new(
            pos,
            ChunkTicketLevel::FULL_CHUNK,
            Some(ChunkTicketLevel::FULL_CHUNK),
            0,
            height,
        ));
        holder.insert_chunk(ChunkAccess::Proto(proto), ChunkStatus::Light);
        holder
    }

    fn empty_holder_with_section_count(pos: ChunkPos, section_count: usize) -> Arc<ChunkHolder> {
        holder_with_sections(
            pos,
            (0..section_count)
                .map(|_| ChunkSection::new_empty())
                .collect(),
        )
    }

    fn horizontal_empty_neighbors(
        center: ChunkPos,
        section_count: usize,
    ) -> Vec<(ChunkPos, Arc<ChunkHolder>)> {
        [
            ChunkPos::new(center.0.x, center.0.y - 1),
            ChunkPos::new(center.0.x, center.0.y + 1),
            ChunkPos::new(center.0.x - 1, center.0.y),
            ChunkPos::new(center.0.x + 1, center.0.y),
        ]
        .into_iter()
        .map(|pos| (pos, empty_holder_with_section_count(pos, section_count)))
        .collect()
    }

    fn roofed_holder(
        pos: ChunkPos,
        section_count: usize,
        roof_section_index: usize,
        roof_local_y: usize,
    ) -> Arc<ChunkHolder> {
        let mut sections = (0..section_count)
            .map(|_| ChunkSection::new_empty())
            .collect::<Vec<_>>();
        for z in 0..16 {
            for x in 0..16 {
                sections[roof_section_index].set_block_state(
                    x,
                    roof_local_y,
                    z,
                    vanilla_blocks::STONE.default_state(),
                );
            }
        }
        holder_with_sections(pos, sections)
    }

    fn roofed_holder_square(
        center: ChunkPos,
        radius: i32,
        section_count: usize,
        roof_section_index: usize,
        roof_local_y: usize,
    ) -> Vec<(ChunkPos, Arc<ChunkHolder>)> {
        let mut holders = Vec::new();
        for z in -radius..=radius {
            for x in -radius..=radius {
                let pos = ChunkPos::new(center.0.x + x, center.0.y + z);
                holders.push((
                    pos,
                    roofed_holder(pos, section_count, roof_section_index, roof_local_y),
                ));
            }
        }
        holders
    }

    fn find_holder(
        holders: &[(ChunkPos, Arc<ChunkHolder>)],
        pos: ChunkPos,
    ) -> Option<Arc<ChunkHolder>> {
        holders
            .iter()
            .find(|(holder_pos, _)| *holder_pos == pos)
            .map(|(_, holder)| Arc::clone(holder))
    }

    fn set_visible_sky_light(
        holder: &ChunkHolder,
        section_y: i32,
        x: usize,
        y: usize,
        z: usize,
        level: u8,
    ) {
        let mut data = LightSectionData::homogeneous(0);
        data.set(x, y, z, level);
        set_sky_light_section(holder, section_y, LightSection::visible(data));
    }

    fn set_sky_light_section(holder: &ChunkHolder, section_y: i32, section: LightSection) {
        let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) else {
            panic!("test chunk should be available");
        };
        let mut light = chunk.light_mut();
        let Some(target) = light.sky.section_mut(section_y) else {
            panic!("test section should be inside light range");
        };
        *target = section;
    }

    fn sky_light_at(holder: &ChunkHolder, pos: BlockPos) -> u8 {
        let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) else {
            panic!("test chunk should be available");
        };
        chunk.light().get_light_value(LightLayer::Sky, pos)
    }

    #[test]
    fn context_requires_sky_layer() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let holder = holder_with_section(center, ChunkSection::new_empty());
        let layout = LightCacheLayout::new(center, range());
        let Ok(workset) = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Inner,
            true,
            |pos| (pos == center).then(|| Arc::clone(&holder)),
            |_| true,
        ) else {
            panic!("relaxed setup should accept missing neighbors");
        };

        workset.with_chunk_read_cache(|chunk_cache| {
            chunk_cache.with_section_read_cache(|section_cache| {
                chunk_cache.with_light_edit(LightLayer::Block, |mut light_edit| {
                    let mut queues = PackedLightPropagationQueues::new();
                    let result = SkyLightPropagationContext::new(
                        section_cache,
                        &mut light_edit,
                        &mut queues,
                    );

                    assert_eq!(
                        result.err(),
                        Some(SkyLightPropagationContextError::WrongLayer {
                            layer: LightLayer::Block,
                        })
                    );
                });
            });
        });
    }

    #[test]
    fn sky_light_chunk_without_edge_checks_propagates_down_air_column() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let mut section = ChunkSection::new_empty();
        section.set_block_state(1, 0, 1, vanilla_blocks::STONE.default_state());
        let holder = holder_with_section(center, section);
        let neighbors = horizontal_empty_neighbors(center, 1);
        let layout = LightCacheLayout::new(center, range());
        let Ok(workset) = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Inner,
            true,
            |pos| {
                if pos == center {
                    Some(Arc::clone(&holder))
                } else {
                    find_holder(&neighbors, pos)
                }
            },
            |_| true,
        ) else {
            panic!("relaxed setup should accept missing neighbors");
        };

        let Ok(result) = propagate_sky_light_chunk_without_edge_checks(&workset) else {
            panic!("matching sky caches should run sky chunk lighting");
        };

        assert!(result.updated_sections.contains(&SectionPos::new(0, 0, 0)));
        assert_eq!(sky_light_at(&holder, BlockPos::new(1, 15, 1)), 15);
        assert_eq!(sky_light_at(&holder, BlockPos::new(1, 1, 1)), 15);
        assert_eq!(sky_light_at(&holder, BlockPos::new(1, 0, 1)), 0);
    }

    #[test]
    fn sky_light_chunk_without_edge_checks_keeps_sealed_roof_dark() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let mut section = ChunkSection::new_empty();
        for z in 0..16 {
            for x in 0..16 {
                section.set_block_state(x, 15, z, vanilla_blocks::STONE.default_state());
            }
        }
        let holder = holder_with_section(center, section);
        let neighbors = roofed_holder_square(center, 2, 1, 0, 15);
        let layout = LightCacheLayout::new(center, range());
        let Ok(workset) = LightWorkset::setup_with_scopes(
            layout,
            LightCacheSetupRadius::Full,
            true,
            |pos| {
                if pos == center {
                    Some(Arc::clone(&holder))
                } else {
                    find_holder(&neighbors, pos)
                }
            },
            |cached_chunk, _, _| (true, cached_chunk.chunk_pos == center),
        ) else {
            panic!("relaxed setup should accept missing neighbors");
        };

        let Ok(result) = propagate_sky_light_chunk_without_edge_checks(&workset) else {
            panic!("matching sky caches should run sky chunk lighting");
        };

        assert!(result.updated_sections.contains(&SectionPos::new(0, 0, 0)));
        assert_eq!(sky_light_at(&holder, BlockPos::new(8, 14, 8)), 0);
        assert_eq!(sky_light_at(&holder, BlockPos::new(8, 15, 8)), 0);
    }

    #[test]
    fn sky_light_changes_add_and_remove_air_column_shadow() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let holder = holder_with_section(center, ChunkSection::new_empty());
        let changed_pos = BlockPos::new(1, 14, 1);
        let layout = LightCacheLayout::new(center, range());

        let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) else {
            panic!("test chunk should be available");
        };
        assert!(
            chunk
                .set_block_state(
                    changed_pos,
                    vanilla_blocks::STONE.default_state(),
                    UpdateFlags::UPDATE_CLIENTS,
                )
                .is_some()
        );
        drop(chunk);

        let Ok(workset) = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Inner,
            true,
            |pos| (pos == center).then(|| Arc::clone(&holder)),
            |_| true,
        ) else {
            panic!("relaxed setup should accept missing neighbors");
        };

        let Ok(result) = propagate_sky_light_changes_with_empty_sections(
            &workset,
            [changed_pos],
            [LightSectionEmptinessChange {
                section_pos: SectionPos::new(0, 0, 0),
                empty: false,
            }],
        ) else {
            panic!("matching sky caches should run sky block changes");
        };

        assert!(result.updated_sections.contains(&SectionPos::new(0, 0, 0)));
        assert_eq!(sky_light_at(&holder, BlockPos::new(1, 15, 1)), 15);
        assert_eq!(sky_light_at(&holder, changed_pos), 0);
        assert_eq!(sky_light_at(&holder, BlockPos::new(1, 13, 1)), 14);

        let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) else {
            panic!("test chunk should be available");
        };
        assert!(
            chunk
                .set_block_state(
                    changed_pos,
                    vanilla_blocks::AIR.default_state(),
                    UpdateFlags::UPDATE_CLIENTS,
                )
                .is_some()
        );
        drop(chunk);

        let Ok(workset) = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Inner,
            true,
            |pos| (pos == center).then(|| Arc::clone(&holder)),
            |_| true,
        ) else {
            panic!("relaxed setup should accept missing neighbors");
        };
        let Ok(result) = propagate_sky_light_changes_with_empty_sections(
            &workset,
            [changed_pos],
            [LightSectionEmptinessChange {
                section_pos: SectionPos::new(0, 0, 0),
                empty: true,
            }],
        ) else {
            panic!("matching sky caches should run sky block changes");
        };

        assert!(result.updated_sections.contains(&SectionPos::new(0, 0, 0)));
        assert_eq!(sky_light_at(&holder, BlockPos::new(1, 15, 1)), 15);
        assert_eq!(sky_light_at(&holder, changed_pos), 15);
        assert_eq!(sky_light_at(&holder, BlockPos::new(1, 13, 1)), 15);
    }

    #[test]
    fn sky_light_chunk_edge_checks_pull_neighbor_under_ceiling() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let east_chunk = ChunkPos::new(1, 0);
        let mut center_section = ChunkSection::new_empty();
        for z in 0..16 {
            for x in 0..16 {
                center_section.set_block_state(x, 15, z, vanilla_blocks::STONE.default_state());
            }
        }
        let center_holder = holder_with_section(center, center_section);
        let east_holder = holder_with_section(east_chunk, ChunkSection::new_empty());
        let neighbors = horizontal_empty_neighbors(center, 1);
        set_visible_sky_light(&east_holder, 0, 0, 14, 1, 15);
        let layout = LightCacheLayout::new(center, range());
        let Ok(workset) = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Inner,
            true,
            |pos| {
                if pos == center {
                    Some(Arc::clone(&center_holder))
                } else if pos == east_chunk {
                    Some(Arc::clone(&east_holder))
                } else {
                    find_holder(&neighbors, pos)
                }
            },
            |_| true,
        ) else {
            panic!("relaxed setup should accept missing neighbors");
        };

        let Ok(result) = propagate_sky_light_chunk(&workset, SkyLightChunkEdgeChecks::Required)
        else {
            panic!("matching sky caches should run sky chunk lighting");
        };

        assert!(result.updated_sections.contains(&SectionPos::new(0, 0, 0)));
        assert_eq!(sky_light_at(&center_holder, BlockPos::new(15, 14, 1)), 14);
        assert_eq!(sky_light_at(&center_holder, BlockPos::new(14, 14, 1)), 13);
        assert_eq!(sky_light_at(&center_holder, BlockPos::new(15, 15, 1)), 0);
    }

    #[test]
    fn sky_light_chunk_requires_center_chunk() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let layout = LightCacheLayout::new(center, range());
        let Ok(workset) = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Inner,
            true,
            |_| None,
            |_| true,
        ) else {
            panic!("relaxed setup should accept missing chunks");
        };

        assert_eq!(
            propagate_sky_light_chunk_without_edge_checks(&workset).err(),
            Some(SkyLightPropagationContextError::MissingCenterChunk { chunk_pos: center })
        );
    }

    #[test]
    fn sky_light_changes_skip_missing_center_chunk() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let layout = LightCacheLayout::new(center, range());
        let Ok(workset) = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Full,
            true,
            |_| None,
            |_| true,
        ) else {
            panic!("relaxed setup should accept missing chunks");
        };

        let Ok(result) = propagate_sky_light_changes_with_empty_sections(
            &workset,
            [BlockPos::new(1, 1, 1)],
            [LightSectionEmptinessChange {
                section_pos: SectionPos::new(0, 0, 0),
                empty: true,
            }],
        ) else {
            panic!("dynamic sky changes should skip a missing center chunk");
        };

        assert!(result.updated_sections.is_empty());
    }
}
