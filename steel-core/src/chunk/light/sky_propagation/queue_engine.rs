use super::{
    BlockPos, BlockStateId, CachedLightBlock, ChunkPos, Direction, LIGHT_BLOCKED,
    LightAxisDirection, LightDirectionSet, LightQueueFlags, MAX_LIGHT_LEVEL, PackedLightQueueEntry,
    SectionPos, SkyLightPropagationContext, get_light_block_into, get_light_opacity,
    light_occlusion_shape, vanilla_blocks,
};

impl SkyLightPropagationContext<'_, '_, '_> {
    pub(super) fn process_delayed_increases(&mut self, entries: &[PackedLightQueueEntry]) {
        for entry in entries {
            let Some(source_block) = self.cached_block_from_entry(*entry) else {
                continue;
            };
            self.light.set(source_block, entry.level());
        }
    }

    pub(super) fn process_delayed_decreases(&mut self, entries: &[PackedLightQueueEntry]) {
        for entry in entries {
            let Some(source_block) = self.cached_block_from_entry(*entry) else {
                continue;
            };
            self.light.set(source_block, 0);
        }
    }

    pub(super) fn get_light_level_extruded(&self, block_pos: BlockPos) -> u8 {
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

    pub(super) fn propagate_neighbor_levels(
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

    pub(super) fn check_chunk_edges(
        &mut self,
        chunk_pos: ChunkPos,
        from_section: i32,
        to_section: i32,
    ) {
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

    pub(super) fn perform_light_increase(&mut self) {
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

    pub(super) fn perform_light_decrease(&mut self) {
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

    pub(super) fn enqueue_decrease(
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

    pub(super) fn enqueue_increase(
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

    pub(super) fn block_state(&self, block_pos: BlockPos) -> BlockStateId {
        let Some(cached_block) = self.layout.cached_block(block_pos) else {
            return Self::air();
        };
        self.sections.get_block_state(cached_block)
    }

    pub(super) const fn current_edge_scan(
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

    pub(super) fn shape_flags(block_state: BlockStateId) -> LightQueueFlags {
        if light_occlusion_shape(block_state).is_empty() {
            LightQueueFlags::EMPTY
        } else {
            LightQueueFlags::EMPTY.with(LightQueueFlags::HAS_SIDED_TRANSPARENT_BLOCKS)
        }
    }

    pub(super) const fn offset(block_pos: BlockPos, direction: LightAxisDirection) -> BlockPos {
        let (dx, dy, dz) = direction.offset();
        block_pos.offset(dx, dy, dz)
    }

    pub(super) fn air() -> BlockStateId {
        vanilla_blocks::AIR.default_state()
    }
}
