use super::*;

impl StructureTemplate {
    pub(crate) fn bounding_box(&self, pos: BlockPos, rotation: Rotation) -> BoundingBox {
        rotation.get_bounding_box(pos.0, self.size)
    }

    pub(crate) fn bounding_box_with_transform(
        &self,
        position: BlockPos,
        rotation: Rotation,
        mirror: StructureMirror,
        pivot: BlockPos,
    ) -> BoundingBox {
        let corner1 = Self::calculate_relative_position(BlockPos::ZERO, mirror, rotation, pivot);
        let corner2 =
            Self::calculate_relative_position(BlockPos(self.size - 1), mirror, rotation, pivot);
        BoundingBox::new(position.0 + corner1.0, position.0 + corner2.0)
    }

    pub(crate) const fn calculate_relative_position(
        pos: BlockPos,
        mirror: StructureMirror,
        rotation: Rotation,
        pivot: BlockPos,
    ) -> BlockPos {
        let (x, z) = match mirror {
            StructureMirror::None => (pos.x(), pos.z()),
            StructureMirror::FrontBack => (-pos.x(), pos.z()),
            StructureMirror::LeftRight => (pos.x(), -pos.z()),
        };
        let pos = rotation.transform_pos(IVec3::new(x, pos.y(), z), pivot.0);
        BlockPos(pos)
    }

    pub(super) fn transform_entity_position(
        pos: DVec3,
        mirror: StructureMirror,
        rotation: Rotation,
        pivot: BlockPos,
    ) -> DVec3 {
        let mut x = pos.x;
        let y = pos.y;
        let mut z = pos.z;
        match mirror {
            StructureMirror::LeftRight => z = 1.0 - z,
            StructureMirror::FrontBack => x = 1.0 - x,
            StructureMirror::None => {}
        }

        let pivot_x = f64::from(pivot.x());
        let pivot_z = f64::from(pivot.z());
        match rotation {
            Rotation::CounterClockwise90 => {
                DVec3::new(pivot_x - pivot_z + z, y, pivot_x + pivot_z + 1.0 - x)
            }
            Rotation::Clockwise90 => {
                DVec3::new(pivot_x + pivot_z + 1.0 - z, y, pivot_z - pivot_x + x)
            }
            Rotation::Clockwise180 => {
                DVec3::new(pivot_x + pivot_x + 1.0 - x, y, pivot_z + pivot_z + 1.0 - z)
            }
            Rotation::None => DVec3::new(x, y, z),
        }
    }

    pub(super) fn transform_entity_rotation(
        (yaw, pitch): (f32, f32),
        mirror: StructureMirror,
        rotation: Rotation,
    ) -> (f32, f32) {
        let yaw = Self::wrap_degrees(yaw);
        let rotated = match rotation {
            Rotation::Clockwise180 => yaw + 180.0,
            Rotation::CounterClockwise90 => yaw + 270.0,
            Rotation::Clockwise90 => yaw + 90.0,
            Rotation::None => yaw,
        };
        let mirrored = match mirror {
            StructureMirror::FrontBack => -yaw,
            StructureMirror::LeftRight => 180.0 - yaw,
            StructureMirror::None => yaw,
        };
        (rotated + mirrored - yaw, pitch)
    }

    pub(super) fn transform_entity_additional_nbt(
        nbt: &mut NbtCompound,
        mirror: StructureMirror,
        rotation: Rotation,
    ) {
        let Some(facing) = Self::entity_facing(nbt) else {
            return;
        };
        let facing = Self::mirror_direction(rotation.rotate(facing), mirror);
        let _ = nbt.remove("Facing");
        nbt.insert("Facing", Self::entity_facing_value(facing));
    }

    pub(super) fn entity_facing(nbt: &NbtCompound) -> Option<Direction> {
        nbt.byte("Facing")
            .map(i32::from)
            .or_else(|| nbt.int("Facing"))
            .and_then(Self::direction_from_entity_facing)
    }

    const fn direction_from_entity_facing(value: i32) -> Option<Direction> {
        match value {
            0 => Some(Direction::Down),
            1 => Some(Direction::Up),
            2 => Some(Direction::North),
            3 => Some(Direction::South),
            4 => Some(Direction::West),
            5 => Some(Direction::East),
            _ => None,
        }
    }

    pub(super) const fn entity_facing_value(direction: Direction) -> i8 {
        match direction {
            Direction::Down => 0,
            Direction::Up => 1,
            Direction::North => 2,
            Direction::South => 3,
            Direction::West => 4,
            Direction::East => 5,
        }
    }

    pub(super) fn wrap_degrees(mut degrees: f32) -> f32 {
        degrees %= 360.0;
        if degrees >= 180.0 {
            degrees -= 360.0;
        }
        if degrees < -180.0 {
            degrees += 360.0;
        }
        degrees
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "structure placement call mirrors vanilla template placement context"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "template placement follows vanilla's single-pass block placement flow"
    )]
    pub(crate) fn place_in_world(
        &self,
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        position: BlockPos,
        reference_pos: BlockPos,
        settings: &StructurePlaceSettings<'_>,
        random: &mut WorldgenRandom,
        flags: UpdateFlags,
    ) -> bool {
        let Some(palette) = self.palette(settings, position, random) else {
            return false;
        };
        if (palette.blocks.is_empty() && self.entities.is_empty())
            || [self.size.x, self.size.y, self.size.z]
                .iter()
                .any(|&axis| axis < 1)
        {
            return false;
        }
        let mut original_blocks = Vec::with_capacity(palette.blocks.len());
        let mut processed_blocks = Vec::with_capacity(palette.blocks.len());

        Self::palette_blocks_for_placement(
            &palette.blocks,
            position,
            settings,
            |block, world_pos| {
                let original = ProcessedBlockInfo {
                    template_pos: block.pos,
                    world_pos: block.pos,
                    state: block.state,
                    nbt: block.nbt.clone(),
                };
                let processed = ProcessedBlockInfo {
                    template_pos: block.pos,
                    world_pos,
                    state: block.state,
                    nbt: block.nbt.clone(),
                };

                if let Some(processed) = Self::process_block(
                    region,
                    registry,
                    &original,
                    processed,
                    settings,
                    reference_pos,
                    random,
                ) {
                    original_blocks.push(original);
                    processed_blocks.push(processed);
                }
            },
        );

        let processed_blocks = Self::finalize_processing(
            region,
            registry,
            position,
            reference_pos,
            settings,
            &original_blocks,
            processed_blocks,
            random,
        );

        let mut placed_any = false;
        let mut placed_positions = Vec::with_capacity(processed_blocks.len());
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut min_z = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;
        let mut max_z = i32::MIN;
        let mut to_fill = Vec::new();
        let mut locked_fluids = Vec::new();
        let apply_waterlogging = settings.liquid_settings == LiquidSettingsData::ApplyWaterlogging;
        for processed in processed_blocks {
            // Always guard placement: the vanilla fallback may enqueue a block outside
            // `bounding_box` for processor/finalize parity without intending a write here.
            if !settings.bounding_box.contains_blockpos(processed.world_pos) {
                continue;
            }

            let final_state = Self::transform_state(
                registry,
                processed.state,
                settings.mirror,
                settings.rotation,
            );
            let previous_fluid_state =
                apply_waterlogging.then(|| Self::fluid_state_at(region, processed.world_pos));
            if processed.nbt.is_some() {
                let barrier_flags = UpdateFlags::UPDATE_INVISIBLE
                    | UpdateFlags::UPDATE_KNOWN_SHAPE
                    | UpdateFlags::UPDATE_SUPPRESS_DROPS
                    | UpdateFlags::UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS
                    | UpdateFlags::UPDATE_SKIP_ON_PLACE;
                let _ = region.set_block_state(
                    processed.world_pos,
                    vanilla_blocks::BARRIER.default_state(),
                    barrier_flags,
                );
            }

            if !region.set_block_state(processed.world_pos, final_state, flags) {
                continue;
            }
            placed_any = true;
            min_x = min_x.min(processed.world_pos.x());
            min_y = min_y.min(processed.world_pos.y());
            min_z = min_z.min(processed.world_pos.z());
            max_x = max_x.max(processed.world_pos.x());
            max_y = max_y.max(processed.world_pos.y());
            max_z = max_z.max(processed.world_pos.z());
            placed_positions.push(processed.world_pos);

            if let Some(mut nbt) = processed.nbt {
                let block_entity_type =
                    Self::block_entity_type_for_nbt_or_state(registry, final_state, &nbt);
                if Self::should_reseed_template_loot(block_entity_type, &nbt) {
                    nbt.insert("LootTableSeed", NbtTag::Long(random.next_i64()));
                }
                Self::place_block_entity(
                    region,
                    processed.world_pos,
                    final_state,
                    block_entity_type,
                    nbt,
                );
            } else {
                let _ = region.remove_block_entity(processed.world_pos);
            }

            if let Some(previous_fluid_state) = previous_fluid_state {
                if Self::fluid_state_for_block(final_state).is_source() {
                    locked_fluids.push(processed.world_pos);
                } else if Self::is_liquid_block_container(final_state) {
                    let _ = Self::place_liquid(
                        region,
                        processed.world_pos,
                        final_state,
                        previous_fluid_state,
                    );
                    if !previous_fluid_state.is_source() {
                        to_fill.push(processed.world_pos);
                    }
                }
            }
        }

        Self::fill_neighbor_source_liquids(region, &mut to_fill, &locked_fluids);

        if placed_any && !flags.contains(UpdateFlags::UPDATE_KNOWN_SHAPE) {
            Self::update_shape_at_edge(
                region,
                flags,
                &placed_positions,
                BlockPos::new(min_x, min_y, min_z),
                BlockPos::new(max_x, max_y, max_z),
            );

            let placed_update_flags =
                (flags & !UpdateFlags::UPDATE_NEIGHBORS) | UpdateFlags::UPDATE_KNOWN_SHAPE;
            for pos in placed_positions {
                let state = region.block_state(pos);
                let new_state = Self::update_from_neighbor_shapes(region, state, pos);
                if state != new_state {
                    let _ = region.set_block_state(pos, new_state, placed_update_flags);
                }
            }
        }

        self.place_entities(region, position, settings);

        true
    }

    pub(super) fn place_entities(
        &self,
        region: &mut WorldGenRegion<'_>,
        position: BlockPos,
        settings: &StructurePlaceSettings<'_>,
    ) {
        if self.entities.is_empty() {
            return;
        }

        let world_offset = DVec3::new(
            f64::from(position.x()),
            f64::from(position.y()),
            f64::from(position.z()),
        );
        for entity in &self.entities {
            let block_pos = Self::calculate_relative_position(
                entity.block_pos,
                settings.mirror,
                settings.rotation,
                settings.rotation_pivot,
            )
            .offset(position.x(), position.y(), position.z());
            if !settings.bounding_box.contains_blockpos(block_pos) {
                continue;
            }

            let pos = Self::transform_entity_position(
                entity.pos,
                settings.mirror,
                settings.rotation,
                settings.rotation_pivot,
            ) + world_offset;
            let rotation = Self::transform_entity_rotation(
                entity.rotation,
                settings.mirror,
                settings.rotation,
            );
            let mut nbt = entity.nbt.clone();
            Self::transform_entity_additional_nbt(&mut nbt, settings.mirror, settings.rotation);

            let mut nbt_bytes = Vec::new();
            nbt.write(&mut nbt_bytes);
            let Ok(nbt) = read_borrowed_compound(&mut Cursor::new(&nbt_bytes)) else {
                log::warn!(
                    "failed to reborrow owned NBT for structure template entity {}",
                    entity.entity_type.key
                );
                continue;
            };

            let runtime_entity = ENTITIES.create_and_load_or_raw(
                EntityLoadRequest {
                    entity_type: entity.entity_type,
                    position: pos,
                    uuid: Uuid::new_v4(),
                    velocity: entity.velocity,
                    rotation,
                    fall_distance: entity.fall_distance,
                    fire_freeze: entity.fire_freeze,
                    on_ground: entity.on_ground,
                    save_data: entity.save_data.clone(),
                    world: region.weak_world(),
                },
                &nbt,
            );
            let _ = region.add_fresh_entity(runtime_entity);
        }
    }

    pub(crate) fn replace_jigsaw_final_states(
        &self,
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        position: BlockPos,
        settings: &StructurePlaceSettings<'_>,
        random: &mut WorldgenRandom,
    ) {
        let Some(palette) = self.palette(settings, position, random) else {
            return;
        };

        for block in &palette.blocks {
            if Self::block_for_state(registry, block.state) != &vanilla_blocks::JIGSAW {
                continue;
            }
            let world_pos = Self::transformed_position(position, block.pos, settings);
            if !settings.bounding_box.contains_blockpos(world_pos) {
                continue;
            }
            let Some(nbt) = block.nbt.as_ref() else {
                continue;
            };
            let final_state = nbt
                .string("final_state")
                .map_or_else(|| "minecraft:air".into(), |value| value.to_str());
            let state = Self::parse_block_state_string(registry, final_state.as_ref())
                .unwrap_or_else(|| vanilla_blocks::AIR.default_state());
            let _ = region.set_block_state(world_pos, state, UpdateFlags::UPDATE_ALL);
        }
    }

    pub(crate) fn data_markers(
        &self,
        registry: &Registry,
        position: BlockPos,
        settings: &StructurePlaceSettings<'_>,
        random: &mut WorldgenRandom,
    ) -> Vec<StructureDataMarker> {
        let Some(palette) = self.palette(settings, position, random) else {
            return Vec::new();
        };

        let mut markers = Vec::new();
        for block in &palette.blocks {
            if Self::block_for_state(registry, block.state) != &vanilla_blocks::STRUCTURE_BLOCK {
                continue;
            }
            let world_pos = Self::transformed_position(position, block.pos, settings);
            if !settings.bounding_box.contains_blockpos(world_pos) {
                continue;
            }
            let Some(nbt) = block.nbt.as_ref() else {
                continue;
            };
            if nbt
                .string("mode")
                .is_none_or(|mode| mode.to_str().as_ref() != "DATA")
            {
                continue;
            }
            let metadata = nbt
                .string("metadata")
                .map(|metadata| metadata.to_str().into_owned())
                .unwrap_or_default();
            markers.push(StructureDataMarker {
                metadata,
                pos: world_pos,
            });
        }
        markers
    }

    pub(super) fn update_shape_at_edge(
        region: &WorldGenRegion<'_>,
        flags: UpdateFlags,
        placed_positions: &[BlockPos],
        min: BlockPos,
        max: BlockPos,
    ) {
        let filled = placed_positions
            .iter()
            .map(|pos| (pos.x() - min.x(), pos.y() - min.y(), pos.z() - min.z()))
            .collect::<BTreeSet<_>>();
        let x_size = max.x() - min.x() + 1;
        let y_size = max.y() - min.y() + 1;
        let z_size = max.z() - min.z() + 1;
        let edge_flags = flags & !UpdateFlags::UPDATE_NEIGHBORS;

        Self::for_all_shape_faces(
            x_size,
            y_size,
            z_size,
            |x, y, z| filled.contains(&(x, y, z)),
            |direction, x, y, z| {
                let pos = min.offset(x, y, z);
                let neighbor_pos = pos.relative(direction);
                let state = region.block_state(pos);
                let neighbor_state = region.block_state(neighbor_pos);
                let new_state = BLOCK_BEHAVIORS
                    .get_behavior(state.get_block())
                    .update_shape(state, region, pos, direction, neighbor_pos, neighbor_state);
                if state != new_state {
                    let _ = region.set_block_state(pos, new_state, edge_flags);
                }

                let new_neighbor_state = BLOCK_BEHAVIORS
                    .get_behavior(neighbor_state.get_block())
                    .update_shape(
                        neighbor_state,
                        region,
                        neighbor_pos,
                        direction.opposite(),
                        pos,
                        new_state,
                    );
                if neighbor_state != new_neighbor_state {
                    let _ = region.set_block_state(neighbor_pos, new_neighbor_state, edge_flags);
                }
            },
        );
    }

    pub(super) fn update_from_neighbor_shapes(
        region: &WorldGenRegion<'_>,
        state: BlockStateId,
        pos: BlockPos,
    ) -> BlockStateId {
        let mut updated = state;
        for direction in Direction::UPDATE_SHAPE_ORDER {
            let neighbor_pos = pos.relative(direction);
            let neighbor_state = region.block_state(neighbor_pos);
            updated = BLOCK_BEHAVIORS
                .get_behavior(updated.get_block())
                .update_shape(
                    updated,
                    region,
                    pos,
                    direction,
                    neighbor_pos,
                    neighbor_state,
                );
        }
        updated
    }

    pub(super) fn fill_neighbor_source_liquids(
        region: &WorldGenRegion<'_>,
        to_fill: &mut Vec<BlockPos>,
        locked_fluids: &[BlockPos],
    ) {
        const DIRECTIONS: [Direction; 5] = [
            Direction::Up,
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
        ];

        let mut filled = true;
        while filled && !to_fill.is_empty() {
            filled = false;
            let mut index = 0;
            while index < to_fill.len() {
                let pos = to_fill[index];
                let mut to_place = Self::fluid_state_at(region, pos);
                for direction in DIRECTIONS {
                    if to_place.is_source() {
                        break;
                    }
                    let neighbor_pos = pos.relative(direction);
                    let neighbor = Self::fluid_state_at(region, neighbor_pos);
                    if neighbor.is_source() && !locked_fluids.contains(&neighbor_pos) {
                        to_place = neighbor;
                    }
                }

                if to_place.is_source() {
                    let state = region.block_state(pos);
                    if Self::is_liquid_block_container(state) {
                        let _ = Self::place_liquid(region, pos, state, to_place);
                        filled = true;
                        to_fill.remove(index);
                        continue;
                    }
                }

                index += 1;
            }
        }
    }

    pub(super) fn fluid_state_at(region: &WorldGenRegion<'_>, pos: BlockPos) -> FluidState {
        Self::fluid_state_for_block(region.block_state(pos))
    }

    pub(super) fn fluid_state_for_block(state: BlockStateId) -> FluidState {
        state.get_fluid_state()
    }

    pub(super) fn is_liquid_block_container(state: BlockStateId) -> bool {
        BLOCK_BEHAVIORS
            .get_behavior(state.get_block())
            .is_liquid_container(state)
    }

    pub(super) fn place_liquid(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        state: BlockStateId,
        fluid_state: FluidState,
    ) -> bool {
        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        behavior.place_liquid(region, pos, state, fluid_state)
    }

    pub(super) fn for_all_shape_faces(
        x_size: i32,
        y_size: i32,
        z_size: i32,
        is_full: impl Fn(i32, i32, i32) -> bool,
        mut consumer: impl FnMut(Direction, i32, i32, i32),
    ) {
        for x in 0..x_size {
            for y in 0..y_size {
                let mut last_full = false;
                for z in 0..=z_size {
                    let full = z != z_size && is_full(x, y, z);
                    if !last_full && full {
                        consumer(Direction::North, x, y, z);
                    }
                    if last_full && !full {
                        consumer(Direction::South, x, y, z - 1);
                    }
                    last_full = full;
                }
            }
        }

        for z in 0..z_size {
            for x in 0..x_size {
                let mut last_full = false;
                for y in 0..=y_size {
                    let full = y != y_size && is_full(x, y, z);
                    if !last_full && full {
                        consumer(Direction::Down, x, y, z);
                    }
                    if last_full && !full {
                        consumer(Direction::Up, x, y - 1, z);
                    }
                    last_full = full;
                }
            }
        }

        for y in 0..y_size {
            for z in 0..z_size {
                let mut last_full = false;
                for x in 0..=x_size {
                    let full = x != x_size && is_full(x, y, z);
                    if !last_full && full {
                        consumer(Direction::West, x, y, z);
                    }
                    if last_full && !full {
                        consumer(Direction::East, x - 1, y, z);
                    }
                    last_full = full;
                }
            }
        }
    }

    pub(super) fn palette(
        &self,
        settings: &StructurePlaceSettings<'_>,
        position: BlockPos,
        random: &mut WorldgenRandom,
    ) -> Option<&StructureTemplatePalette> {
        if self.palettes.is_empty() {
            return None;
        }
        let Ok(bound) = i32::try_from(self.palettes.len()) else {
            panic!(
                "structure template palette count {} exceeds i32 range",
                self.palettes.len()
            );
        };
        let index = match settings.processor_random {
            StructureProcessorRandom::Placement => random.next_i32_bounded(bound),
            StructureProcessorRandom::Positional => {
                let mut random = LegacyRandom::from_seed(Self::block_pos_seed(position) as u64);
                random.next_i32_bounded(bound)
            }
        };
        Some(&self.palettes[index as usize])
    }

    pub(super) fn place_block_entity(
        region: &mut WorldGenRegion<'_>,
        pos: BlockPos,
        state: BlockStateId,
        block_entity_type: Option<BlockEntityTypeRef>,
        nbt: NbtCompound,
    ) {
        let Some(block_entity_type) = block_entity_type else {
            return;
        };
        let _ = region.set_block_entity_data(pos, block_entity_type, state, nbt);
    }

    pub(super) fn block_entity_type_for_nbt_or_state(
        registry: &Registry,
        state: BlockStateId,
        nbt: &NbtCompound,
    ) -> Option<BlockEntityTypeRef> {
        if let Some(id) = nbt.string("id") {
            let id = Identifier::from_str(id.to_str().as_ref()).ok()?;
            return registry.block_entity_types.by_key(&id);
        }
        Self::block_entity_type_for_state(registry, state)
    }

    pub(super) fn block_entity_type_for_state(
        registry: &Registry,
        state: BlockStateId,
    ) -> Option<BlockEntityTypeRef> {
        let block = Self::block_for_state(registry, state);
        if block == &vanilla_blocks::SUSPICIOUS_SAND || block == &vanilla_blocks::SUSPICIOUS_GRAVEL
        {
            return Some(&vanilla_block_entity_types::BRUSHABLE_BLOCK);
        }
        None
    }

    pub(super) fn should_reseed_template_loot(
        block_entity_type: Option<BlockEntityTypeRef>,
        nbt: &NbtCompound,
    ) -> bool {
        nbt.contains("LootTable")
            && block_entity_type.is_some_and(Self::is_randomizable_container_block_entity)
    }

    pub(super) fn is_randomizable_container_block_entity(
        block_entity_type: BlockEntityTypeRef,
    ) -> bool {
        let key = &block_entity_type.key;
        key == &vanilla_block_entity_types::BARREL.key
            || key == &vanilla_block_entity_types::CHEST.key
            || key == &vanilla_block_entity_types::TRAPPED_CHEST.key
            || key == &vanilla_block_entity_types::DISPENSER.key
            || key == &vanilla_block_entity_types::DROPPER.key
            || key == &vanilla_block_entity_types::HOPPER.key
            || key == &vanilla_block_entity_types::SHULKER_BOX.key
            || key == &vanilla_block_entity_types::CRAFTER.key
    }
}
