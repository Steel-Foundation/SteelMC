use super::*;

impl StructureTemplate {
    /// `StructureLayoutOptimizer`: skip out-of-bounds blocks before processors run.
    /// Disabled when a `Capped` processor is present — it needs the full block list
    /// in `finalize_processing` (Trail Ruins).
    pub(super) fn pre_filters_placement_bounds(processors: &[StructureProcessorKind]) -> bool {
        !processors
            .iter()
            .any(|processor| matches!(processor, StructureProcessorKind::Capped { .. }))
    }

    pub(super) fn palette_blocks_for_placement<F: FnMut(&StructureBlockInfo, BlockPos)>(
        blocks: &[StructureBlockInfo],
        position: BlockPos,
        settings: &StructurePlaceSettings<'_>,
        mut f: F,
    ) {
        if !Self::pre_filters_placement_bounds(settings.processors) {
            for block in blocks {
                f(
                    block,
                    Self::transformed_position(position, block.pos, settings),
                );
            }
            return;
        }

        for block in blocks {
            let world_pos = Self::transformed_position(position, block.pos, settings);
            if settings.bounding_box.contains_blockpos(world_pos) {
                f(block, world_pos);
            }
        }
    }

    pub(super) const fn transformed_position(
        position: BlockPos,
        template_pos: BlockPos,
        settings: &StructurePlaceSettings<'_>,
    ) -> BlockPos {
        let transformed = Self::calculate_relative_position(
            template_pos,
            settings.mirror,
            settings.rotation,
            settings.rotation_pivot,
        );
        position.offset(transformed.x(), transformed.y(), transformed.z())
    }

    pub(super) fn process_block(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        original: &ProcessedBlockInfo,
        initial: ProcessedBlockInfo,
        settings: &StructurePlaceSettings<'_>,
        reference_pos: BlockPos,
        random: &mut WorldgenRandom,
    ) -> Option<ProcessedBlockInfo> {
        let mut current = initial;
        if settings.block_ignore.ignores(registry, current.state) {
            return None;
        }

        if settings.replace_jigsaws {
            current = Self::replace_jigsaw_block(registry, current)?;
        }

        for processor in settings.processors {
            current = Self::process_block_with_processor(
                region,
                registry,
                processor,
                original,
                current,
                settings,
                reference_pos,
                random,
            )?;
        }
        if settings.projection == Some(Projection::TerrainMatching) {
            current = Self::apply_terrain_matching_projection(region, original, current);
        }
        if settings.late_block_ignore.ignores(registry, current.state) {
            return None;
        }
        Some(current)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "processor calls mirror vanilla StructureProcessor.processBlock inputs"
    )]
    pub(super) fn process_block_with_processor(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        processor: &StructureProcessorKind,
        original: &ProcessedBlockInfo,
        current: ProcessedBlockInfo,
        settings: &StructurePlaceSettings<'_>,
        reference_pos: BlockPos,
        random: &mut WorldgenRandom,
    ) -> Option<ProcessedBlockInfo> {
        match processor {
            StructureProcessorKind::BlockRot {
                rottable_blocks,
                integrity,
            } => {
                if rottable_blocks.as_ref().is_some_and(|tag| {
                    !registry
                        .blocks
                        .is_in_tag(Self::block_for_state(registry, original.state), tag)
                }) {
                    return Some(current);
                }
                (Self::processor_next_f32(settings, current.world_pos, random) <= *integrity)
                    .then_some(current)
            }
            StructureProcessorKind::ProtectedBlocks { cannot_replace } => {
                let existing =
                    Self::block_for_state(registry, region.block_state(current.world_pos));
                (!existing.has_tag(cannot_replace)).then_some(current)
            }
            StructureProcessorKind::Rule { rules } => {
                let mut rule_random =
                    LegacyRandom::from_seed(Self::block_pos_seed(current.world_pos) as u64);
                let location_state = region.block_state(current.world_pos);
                for rule in rules {
                    if Self::rule_matches(
                        registry,
                        rule,
                        current.state,
                        location_state,
                        original.template_pos,
                        current.world_pos,
                        reference_pos,
                        &mut rule_random,
                    ) {
                        return Some(Self::apply_rule(registry, rule, current, &mut rule_random));
                    }
                }
                Some(current)
            }
            StructureProcessorKind::BlockAge { mossiness } => Some(Self::process_block_age(
                registry, current, *mossiness, settings, random,
            )),
            StructureProcessorKind::LavaSubmergedBlock => Some(Self::process_lava_submerged_block(
                registry,
                region.block_state(current.world_pos),
                current,
            )),
            StructureProcessorKind::BlackstoneReplace => {
                Some(Self::process_blackstone_replace(registry, current))
            }
            StructureProcessorKind::Capped { .. } => Some(current),
        }
    }

    pub(super) fn process_block_age(
        registry: &Registry,
        current: ProcessedBlockInfo,
        mossiness: f32,
        settings: &StructurePlaceSettings<'_>,
        random: &mut WorldgenRandom,
    ) -> ProcessedBlockInfo {
        match settings.processor_random {
            StructureProcessorRandom::Placement => {
                Self::process_block_age_with_random(registry, current, mossiness, random)
            }
            StructureProcessorRandom::Positional => {
                let mut random =
                    LegacyRandom::from_seed(Self::block_pos_seed(current.world_pos) as u64);
                Self::process_block_age_with_random(registry, current, mossiness, &mut random)
            }
        }
    }

    pub(super) fn process_block_age_with_random(
        registry: &Registry,
        mut current: ProcessedBlockInfo,
        mossiness: f32,
        random: &mut impl Random,
    ) -> ProcessedBlockInfo {
        let block = Self::block_for_state(registry, current.state);
        let new_state = if block == &vanilla_blocks::STONE_BRICKS
            || block == &vanilla_blocks::STONE
            || block == &vanilla_blocks::CHISELED_STONE_BRICKS
        {
            Self::maybe_replace_full_stone_block(registry, mossiness, random)
        } else if block.has_tag(&BlockTag::STAIRS) {
            Self::maybe_replace_stairs(registry, current.state, mossiness, random)
        } else if block.has_tag(&BlockTag::SLABS) {
            Self::maybe_replace_slab(registry, current.state, mossiness, random)
        } else if block.has_tag(&BlockTag::WALLS) {
            Self::maybe_replace_wall(registry, current.state, mossiness, random)
        } else if block == &vanilla_blocks::OBSIDIAN {
            Self::maybe_replace_obsidian(registry, random)
        } else {
            None
        };

        if let Some(new_state) = new_state {
            current.state = new_state;
        }
        current
    }

    pub(super) fn maybe_replace_full_stone_block(
        registry: &Registry,
        mossiness: f32,
        random: &mut impl Random,
    ) -> Option<BlockStateId> {
        if random.next_f32() >= 0.5 {
            return None;
        }

        let non_mossy = [
            registry
                .blocks
                .get_default_state_id(&vanilla_blocks::CRACKED_STONE_BRICKS),
            Self::random_facing_stairs(registry, &vanilla_blocks::STONE_BRICK_STAIRS, random),
        ];
        let mossy = [
            registry
                .blocks
                .get_default_state_id(&vanilla_blocks::MOSSY_STONE_BRICKS),
            Self::random_facing_stairs(registry, &vanilla_blocks::MOSSY_STONE_BRICK_STAIRS, random),
        ];
        let candidates = if random.next_f32() < mossiness {
            mossy
        } else {
            non_mossy
        };
        Some(candidates[random.next_i32_bounded(2) as usize])
    }

    pub(super) fn maybe_replace_stairs(
        registry: &Registry,
        state: BlockStateId,
        mossiness: f32,
        random: &mut impl Random,
    ) -> Option<BlockStateId> {
        if random.next_f32() >= 0.5 {
            return None;
        }

        let non_mossy = [
            registry
                .blocks
                .get_default_state_id(&vanilla_blocks::STONE_SLAB),
            registry
                .blocks
                .get_default_state_id(&vanilla_blocks::STONE_BRICK_SLAB),
        ];
        let mossy = [
            registry
                .blocks
                .copy_matching_properties(state, &vanilla_blocks::MOSSY_STONE_BRICK_STAIRS),
            registry
                .blocks
                .get_default_state_id(&vanilla_blocks::MOSSY_STONE_BRICK_SLAB),
        ];
        let candidates = if random.next_f32() < mossiness {
            mossy
        } else {
            non_mossy
        };
        Some(candidates[random.next_i32_bounded(2) as usize])
    }

    pub(super) fn maybe_replace_slab(
        registry: &Registry,
        state: BlockStateId,
        mossiness: f32,
        random: &mut impl Random,
    ) -> Option<BlockStateId> {
        (random.next_f32() < mossiness).then(|| {
            registry
                .blocks
                .copy_matching_properties(state, &vanilla_blocks::MOSSY_STONE_BRICK_SLAB)
        })
    }

    pub(super) fn maybe_replace_wall(
        registry: &Registry,
        state: BlockStateId,
        mossiness: f32,
        random: &mut impl Random,
    ) -> Option<BlockStateId> {
        (random.next_f32() < mossiness).then(|| {
            registry
                .blocks
                .copy_matching_properties(state, &vanilla_blocks::MOSSY_STONE_BRICK_WALL)
        })
    }

    pub(super) fn maybe_replace_obsidian(
        registry: &Registry,
        random: &mut impl Random,
    ) -> Option<BlockStateId> {
        (random.next_f32() < 0.15).then(|| {
            registry
                .blocks
                .get_default_state_id(&vanilla_blocks::CRYING_OBSIDIAN)
        })
    }

    pub(super) fn random_facing_stairs(
        registry: &Registry,
        block: BlockRef,
        random: &mut impl Random,
    ) -> BlockStateId {
        const HORIZONTAL_DIRECTIONS: [BlockPropertyDirection; 4] = [
            BlockPropertyDirection::North,
            BlockPropertyDirection::East,
            BlockPropertyDirection::South,
            BlockPropertyDirection::West,
        ];

        let facing = HORIZONTAL_DIRECTIONS[random.next_i32_bounded(4) as usize];
        let half = if random.next_i32_bounded(2) == 0 {
            Half::Top
        } else {
            Half::Bottom
        };
        let state = registry.blocks.get_default_state_id(block);
        let state = registry
            .blocks
            .set_property(state, &BlockStateProperties::FACING, facing);
        registry
            .blocks
            .set_property(state, &BlockStateProperties::HALF, half)
    }

    pub(super) fn process_lava_submerged_block(
        registry: &Registry,
        existing_state: BlockStateId,
        mut current: ProcessedBlockInfo,
    ) -> ProcessedBlockInfo {
        if Self::block_for_state(registry, existing_state) == &vanilla_blocks::LAVA
            && !blocks::shapes::is_offset_shape_full_block(
                registry
                    .blocks
                    .get_outline_shape_at(current.state, current.world_pos),
            )
        {
            current.state = registry.blocks.get_default_state_id(&vanilla_blocks::LAVA);
        }
        current
    }

    pub(super) fn process_blackstone_replace(
        registry: &Registry,
        mut current: ProcessedBlockInfo,
    ) -> ProcessedBlockInfo {
        let Some(block) =
            Self::blackstone_replacement_block(Self::block_for_state(registry, current.state))
        else {
            return current;
        };

        let mut new_state = registry.blocks.get_default_state_id(block);
        if let Some(facing) = registry
            .blocks
            .try_get_property(current.state, &BlockStateProperties::FACING)
            && registry
                .blocks
                .try_get_property(new_state, &BlockStateProperties::FACING)
                .is_some()
        {
            new_state =
                registry
                    .blocks
                    .set_property(new_state, &BlockStateProperties::FACING, facing);
        }
        if let Some(half) = registry
            .blocks
            .try_get_property(current.state, &BlockStateProperties::HALF)
            && registry
                .blocks
                .try_get_property(new_state, &BlockStateProperties::HALF)
                .is_some()
        {
            new_state = registry
                .blocks
                .set_property(new_state, &BlockStateProperties::HALF, half);
        }
        if let Some(slab_type) = registry
            .blocks
            .try_get_property(current.state, &BlockStateProperties::SLAB_TYPE)
            && registry
                .blocks
                .try_get_property(new_state, &BlockStateProperties::SLAB_TYPE)
                .is_some()
        {
            new_state = registry.blocks.set_property(
                new_state,
                &BlockStateProperties::SLAB_TYPE,
                slab_type,
            );
        }

        current.state = new_state;
        current
    }

    pub(super) fn blackstone_replacement_block(block: BlockRef) -> Option<BlockRef> {
        if block == &vanilla_blocks::COBBLESTONE || block == &vanilla_blocks::MOSSY_COBBLESTONE {
            Some(&vanilla_blocks::BLACKSTONE)
        } else if block == &vanilla_blocks::STONE {
            Some(&vanilla_blocks::POLISHED_BLACKSTONE)
        } else if block == &vanilla_blocks::STONE_BRICKS
            || block == &vanilla_blocks::MOSSY_STONE_BRICKS
        {
            Some(&vanilla_blocks::POLISHED_BLACKSTONE_BRICKS)
        } else if block == &vanilla_blocks::COBBLESTONE_STAIRS
            || block == &vanilla_blocks::MOSSY_COBBLESTONE_STAIRS
        {
            Some(&vanilla_blocks::BLACKSTONE_STAIRS)
        } else if block == &vanilla_blocks::STONE_STAIRS {
            Some(&vanilla_blocks::POLISHED_BLACKSTONE_STAIRS)
        } else if block == &vanilla_blocks::STONE_BRICK_STAIRS
            || block == &vanilla_blocks::MOSSY_STONE_BRICK_STAIRS
        {
            Some(&vanilla_blocks::POLISHED_BLACKSTONE_BRICK_STAIRS)
        } else if block == &vanilla_blocks::COBBLESTONE_SLAB
            || block == &vanilla_blocks::MOSSY_COBBLESTONE_SLAB
        {
            Some(&vanilla_blocks::BLACKSTONE_SLAB)
        } else if block == &vanilla_blocks::SMOOTH_STONE_SLAB
            || block == &vanilla_blocks::STONE_SLAB
        {
            Some(&vanilla_blocks::POLISHED_BLACKSTONE_SLAB)
        } else if block == &vanilla_blocks::STONE_BRICK_SLAB
            || block == &vanilla_blocks::MOSSY_STONE_BRICK_SLAB
        {
            Some(&vanilla_blocks::POLISHED_BLACKSTONE_BRICK_SLAB)
        } else if block == &vanilla_blocks::STONE_BRICK_WALL
            || block == &vanilla_blocks::MOSSY_STONE_BRICK_WALL
        {
            Some(&vanilla_blocks::POLISHED_BLACKSTONE_BRICK_WALL)
        } else if block == &vanilla_blocks::COBBLESTONE_WALL
            || block == &vanilla_blocks::MOSSY_COBBLESTONE_WALL
        {
            Some(&vanilla_blocks::BLACKSTONE_WALL)
        } else if block == &vanilla_blocks::CHISELED_STONE_BRICKS {
            Some(&vanilla_blocks::CHISELED_POLISHED_BLACKSTONE)
        } else if block == &vanilla_blocks::CRACKED_STONE_BRICKS {
            Some(&vanilla_blocks::CRACKED_POLISHED_BLACKSTONE_BRICKS)
        } else if block == &vanilla_blocks::IRON_BARS {
            Some(&vanilla_blocks::IRON_CHAIN)
        } else {
            None
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "processor finalization receives vanilla's full template processing context"
    )]
    pub(super) fn finalize_processing(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        position: BlockPos,
        reference_pos: BlockPos,
        settings: &StructurePlaceSettings<'_>,
        original_blocks: &[ProcessedBlockInfo],
        mut processed_blocks: Vec<ProcessedBlockInfo>,
        random: &mut WorldgenRandom,
    ) -> Vec<ProcessedBlockInfo> {
        for processor in settings.processors {
            if let StructureProcessorKind::Capped { delegate, limit } = processor {
                processed_blocks = Self::finalize_capped_processing(
                    region,
                    registry,
                    position,
                    reference_pos,
                    delegate,
                    limit,
                    original_blocks,
                    processed_blocks,
                    settings,
                    random,
                );
            }
        }
        processed_blocks
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "matches vanilla CappedProcessor.finalizeProcessing inputs"
    )]
    pub(super) fn finalize_capped_processing(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        position: BlockPos,
        reference_pos: BlockPos,
        delegate: &StructureProcessorKind,
        limit: &IntProvider,
        original_blocks: &[ProcessedBlockInfo],
        mut processed_blocks: Vec<ProcessedBlockInfo>,
        settings: &StructurePlaceSettings<'_>,
        random: &mut WorldgenRandom,
    ) -> Vec<ProcessedBlockInfo> {
        if limit.max() == 0 || processed_blocks.is_empty() {
            return processed_blocks;
        }
        if original_blocks.len() != processed_blocks.len() {
            return processed_blocks;
        }

        let Ok(processed_len_i32) = i32::try_from(processed_blocks.len()) else {
            panic!(
                "processed structure block list length {} exceeds i32 range",
                processed_blocks.len()
            );
        };

        let mut cap_random = Self::capped_processor_random(region.seed(), position);
        let max_to_replace = limit.sample(&mut cap_random).min(processed_len_i32);
        if max_to_replace < 1 {
            return processed_blocks;
        }

        let mut indices = (0..processed_blocks.len()).collect::<Vec<_>>();
        Self::vanilla_shuffle(&mut indices, &mut cap_random);

        let mut replaced = 0;
        for index in indices {
            if replaced >= max_to_replace {
                break;
            }

            let current = processed_blocks[index].clone();
            let Some(altered) = Self::process_block_with_processor(
                region,
                registry,
                delegate,
                &original_blocks[index],
                current,
                settings,
                reference_pos,
                random,
            ) else {
                continue;
            };

            if altered != processed_blocks[index] {
                processed_blocks[index] = altered;
                replaced += 1;
            }
        }

        processed_blocks
    }

    pub(super) fn processor_next_f32(
        settings: &StructurePlaceSettings<'_>,
        pos: BlockPos,
        random: &mut WorldgenRandom,
    ) -> f32 {
        match settings.processor_random {
            StructureProcessorRandom::Placement => random.next_f32(),
            StructureProcessorRandom::Positional => {
                let mut random = LegacyRandom::from_seed(Self::block_pos_seed(pos) as u64);
                random.next_f32()
            }
        }
    }

    pub(super) fn capped_processor_random(world_seed: i64, position: BlockPos) -> RandomSource {
        LegacyRandom::from_seed(world_seed as u64)
            .next_positional()
            .at(position.x(), position.y(), position.z())
    }

    pub(super) fn vanilla_shuffle<T>(items: &mut [T], random: &mut impl Random) {
        for i in (1..items.len()).rev() {
            let Ok(bound) = i32::try_from(i + 1) else {
                panic!(
                    "structure processor shuffle length {} exceeds i32 range",
                    items.len()
                );
            };
            let j = random.next_i32_bounded(bound) as usize;
            items.swap(i, j);
        }
    }

    pub(super) fn replace_jigsaw_block(
        registry: &Registry,
        mut current: ProcessedBlockInfo,
    ) -> Option<ProcessedBlockInfo> {
        if Self::block_for_state(registry, current.state) != &vanilla_blocks::JIGSAW {
            return Some(current);
        }

        let Some(nbt) = current.nbt.as_ref() else {
            return Some(current);
        };
        let final_state = nbt
            .string("final_state")
            .map_or_else(|| "minecraft:air".into(), |value| value.to_str());
        current.state = Self::parse_block_state_string(registry, final_state.as_ref())
            .unwrap_or_else(|| vanilla_blocks::AIR.default_state());
        current.nbt = None;

        (Self::block_for_state(registry, current.state) != &vanilla_blocks::STRUCTURE_VOID)
            .then_some(current)
    }

    pub(super) fn parse_block_state_string(
        registry: &Registry,
        value: &str,
    ) -> Option<BlockStateId> {
        let (name, rest) = Self::read_block_identifier_prefix(value)?;
        let id = Identifier::from_str(name).ok()?;
        let block = registry.blocks.by_key(&id)?;

        let mut parsed_properties = Vec::new();
        if rest.starts_with('[') {
            let properties = Self::read_block_state_properties_prefix(rest)?;
            if !properties.is_empty() {
                for property in properties.split(',') {
                    let (key, value) = property.split_once('=')?;
                    parsed_properties.push((key, value));
                }
            }
        }

        registry
            .blocks
            .state_id_from_block_defaulted_properties(block, parsed_properties)
    }

    pub(super) fn read_block_identifier_prefix(value: &str) -> Option<(&str, &str)> {
        let end = value
            .char_indices()
            .find_map(|(index, char)| {
                (char != ':' && !Identifier::valid_char(char)).then_some(index)
            })
            .unwrap_or(value.len());
        (end > 0).then_some((&value[..end], &value[end..]))
    }

    pub(super) fn read_block_state_properties_prefix(rest: &str) -> Option<&str> {
        let rest = rest.strip_prefix('[')?;
        let end = rest.find(']')?;
        Some(&rest[..end])
    }

    pub(super) fn apply_terrain_matching_projection(
        region: &WorldGenRegion<'_>,
        original: &ProcessedBlockInfo,
        mut current: ProcessedBlockInfo,
    ) -> ProcessedBlockInfo {
        let height = region.height_at(
            HeightmapType::WorldSurfaceWg,
            current.world_pos.x(),
            current.world_pos.z(),
        ) - 1;
        current.world_pos = BlockPos::new(
            current.world_pos.x(),
            height + original.template_pos.y(),
            current.world_pos.z(),
        );
        current
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "processor rules receive the same state and position tuple as vanilla"
    )]
    pub(super) fn rule_matches(
        registry: &Registry,
        rule: &ProcessorRuleData,
        input_state: BlockStateId,
        location_state: BlockStateId,
        template_pos: BlockPos,
        world_pos: BlockPos,
        reference_pos: BlockPos,
        random: &mut LegacyRandom,
    ) -> bool {
        Self::rule_test_matches(registry, &rule.input_predicate, input_state, random)
            && Self::rule_test_matches(registry, &rule.location_predicate, location_state, random)
            && Self::pos_rule_test_matches(
                &rule.position_predicate,
                template_pos,
                world_pos,
                reference_pos,
                random,
            )
    }

    pub(super) fn rule_test_matches(
        registry: &Registry,
        test: &StructureRuleTestData,
        state: BlockStateId,
        random: &mut LegacyRandom,
    ) -> bool {
        match test {
            StructureRuleTestData::AlwaysTrue => true,
            StructureRuleTestData::BlockMatch { block } => registry
                .blocks
                .by_key(block)
                .is_some_and(|block_ref| Self::block_for_state(registry, state) == block_ref),
            StructureRuleTestData::RandomBlockMatch { block, probability } => {
                registry
                    .blocks
                    .by_key(block)
                    .is_some_and(|block_ref| Self::block_for_state(registry, state) == block_ref)
                    && random.next_f32() < *probability
            }
            StructureRuleTestData::TagMatch { tag } => registry
                .blocks
                .is_in_tag(Self::block_for_state(registry, state), tag),
            StructureRuleTestData::BlockStateMatch { block_state } => {
                state
                    == WorldgenStateResolver::block_state_from_data(
                        registry,
                        block_state,
                        "structure processor block-state predicate",
                    )
            }
        }
    }

    pub(super) fn pos_rule_test_matches(
        test: &PosRuleTestData,
        _template_pos: BlockPos,
        world_pos: BlockPos,
        reference_pos: BlockPos,
        random: &mut LegacyRandom,
    ) -> bool {
        match test {
            PosRuleTestData::AlwaysTrue => true,
            PosRuleTestData::AxisAlignedLinearPos {
                axis,
                min_chance,
                max_chance,
                min_dist,
                max_dist,
            } => {
                let dist = match axis {
                    StructureProcessorAxis::X => (world_pos.x() - reference_pos.x()).abs(),
                    StructureProcessorAxis::Y => (world_pos.y() - reference_pos.y()).abs(),
                    StructureProcessorAxis::Z => (world_pos.z() - reference_pos.z()).abs(),
                };
                random.next_f32()
                    <= Self::clamped_lerp_inverse(
                        dist,
                        *min_dist,
                        *max_dist,
                        *min_chance,
                        *max_chance,
                    )
            }
        }
    }

    pub(super) fn apply_rule(
        registry: &Registry,
        rule: &ProcessorRuleData,
        mut current: ProcessedBlockInfo,
        random: &mut LegacyRandom,
    ) -> ProcessedBlockInfo {
        current.state = WorldgenStateResolver::block_state_from_data(
            registry,
            &rule.output_state,
            "structure processor output state",
        );
        current.nbt = match &rule.block_entity_modifier {
            RuleBlockEntityModifierData::Passthrough => current.nbt,
            RuleBlockEntityModifierData::AppendLoot { loot_table } => {
                let mut nbt = current.nbt.unwrap_or_default();
                nbt.insert("LootTable", NbtTag::String(loot_table.to_string().into()));
                nbt.insert("LootTableSeed", NbtTag::Long(random.next_i64()));
                Some(nbt)
            }
        };
        current
    }
}
