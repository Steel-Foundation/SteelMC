use super::{
    BlockPos, BlockStateExt, ChunkPos, Direction, LIGHT_BLOCKED, LightAxisDirection,
    LightDirectionSet, LightQueueFlags, MAX_LIGHT_LEVEL, PackedLightQueueEntry, SectionPos,
    SkyLightChunkEdgeChecks, SkyLightPropagationContext, get_light_block_into, get_light_opacity,
};

impl SkyLightPropagationContext<'_, '_, '_> {
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

    pub(super) fn init_light_section(
        &mut self,
        section_pos: SectionPos,
        extrude: bool,
        init_removed: bool,
    ) {
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

    pub(super) fn section_is_non_empty(&self, section_pos: SectionPos) -> bool {
        if let Some(empty) = self.sections.section_empty(section_pos) {
            return !empty;
        }

        if let Some(empty) = self.light.section_empty(section_pos) {
            return !empty;
        }

        self.sections.has_non_empty_section(section_pos)
    }

    pub(super) fn check_missing_section(
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

        for z in 0..super::super::CHUNK_EDGE {
            for x in 0..super::super::CHUNK_EDGE {
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
}
