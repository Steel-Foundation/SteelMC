use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

#[derive(Clone, Copy)]
enum MultifaceSpreadType {
    SamePosition,
    SamePlane,
    WrapAround,
}

struct MultifaceSpreadPos {
    pos: BlockPos,
    face: Direction,
}

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_multiface_growth_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &MultifaceGrowthConfiguration,
        origin: BlockPos,
    ) -> bool {
        let origin_state = region.block_state(origin);
        if !Self::multiface_is_air_or_water(origin_state) {
            return false;
        }

        let search_directions = Self::multiface_shuffled_valid_directions(random, config);
        if Self::place_multiface_growth_if_possible(
            region,
            registry,
            random,
            origin,
            origin_state,
            config,
            &search_directions,
        ) {
            return true;
        }

        for search_direction in &search_directions {
            let placement_directions = Self::multiface_shuffled_valid_directions_except(
                random,
                config,
                search_direction.opposite(),
            );

            for _ in 0..config.search_range {
                let pos = origin.relative(*search_direction);
                let state = region.block_state(pos);
                if !Self::multiface_is_air_or_water(state)
                    && !Self::multiface_is_place_block(registry, config, state)
                {
                    break;
                }

                if Self::place_multiface_growth_if_possible(
                    region,
                    registry,
                    random,
                    pos,
                    state,
                    config,
                    &placement_directions,
                ) {
                    return true;
                }
            }
        }

        false
    }

    fn place_multiface_growth_if_possible(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        pos: BlockPos,
        old_state: BlockStateId,
        config: &MultifaceGrowthConfiguration,
        placement_directions: &[Direction],
    ) -> bool {
        for placement_direction in placement_directions {
            let neighbor_state = region.block_state(pos.relative(*placement_direction));
            if !Self::multiface_can_be_placed_on(registry, config, neighbor_state) {
                continue;
            }

            let Some(new_state) = Self::multiface_state_for_placement(
                region,
                registry,
                config,
                old_state,
                pos,
                *placement_direction,
            ) else {
                return false;
            };

            let _ = region.set_block_state(pos, new_state, UpdateFlags::UPDATE_ALL);
            region.mark_pos_for_postprocessing(pos);
            if random.next_f32() < config.chance_of_spreading {
                let _ = Self::spread_multiface_from_face_toward_random_direction(
                    region,
                    registry,
                    random,
                    config,
                    new_state,
                    pos,
                    *placement_direction,
                    true,
                );
            }

            return true;
        }

        false
    }

    fn spread_multiface_from_face_toward_random_direction(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &MultifaceGrowthConfiguration,
        state: BlockStateId,
        pos: BlockPos,
        starting_face: Direction,
        post_process: bool,
    ) -> Option<MultifaceSpreadPos> {
        let directions = Self::shuffled_directions(random, Self::VANILLA_DIRECTION_VALUES);
        for spread_direction in directions {
            if let Some(spread_pos) = Self::spread_multiface_from_face_toward_direction(
                region,
                registry,
                config,
                state,
                pos,
                starting_face,
                spread_direction,
                post_process,
            ) {
                return Some(spread_pos);
            }
        }

        None
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Mirrors vanilla MultifaceSpreader"
    )]
    fn spread_multiface_from_face_toward_direction(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        config: &MultifaceGrowthConfiguration,
        state: BlockStateId,
        pos: BlockPos,
        starting_face: Direction,
        spread_direction: Direction,
        post_process: bool,
    ) -> Option<MultifaceSpreadPos> {
        let spread_pos = Self::multiface_spread_from_face_toward_direction(
            region,
            registry,
            config,
            state,
            pos,
            starting_face,
            spread_direction,
        )?;
        if Self::spread_multiface_to_face(region, registry, config, &spread_pos, post_process) {
            Some(spread_pos)
        } else {
            None
        }
    }

    fn multiface_spread_from_face_toward_direction(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        config: &MultifaceGrowthConfiguration,
        state: BlockStateId,
        pos: BlockPos,
        starting_face: Direction,
        spread_direction: Direction,
    ) -> Option<MultifaceSpreadPos> {
        if spread_direction.axis() == starting_face.axis() {
            return None;
        }

        if !Self::multiface_is_other_block_valid_as_source(registry, config, state)
            && (!Self::multiface_has_face(state, starting_face)
                || Self::multiface_has_face(state, spread_direction))
        {
            return None;
        }

        for spread_type in Self::multiface_spread_types(config) {
            let spread_pos =
                Self::multiface_spread_pos(pos, spread_direction, starting_face, spread_type);
            if Self::multiface_can_spread_into(region, registry, config, pos, &spread_pos) {
                return Some(spread_pos);
            }
        }

        None
    }

    fn spread_multiface_to_face(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        config: &MultifaceGrowthConfiguration,
        spread_pos: &MultifaceSpreadPos,
        post_process: bool,
    ) -> bool {
        let old_state = region.block_state(spread_pos.pos);
        let Some(spread_state) = Self::multiface_state_for_placement(
            region,
            registry,
            config,
            old_state,
            spread_pos.pos,
            spread_pos.face,
        ) else {
            return false;
        };

        if post_process {
            region.mark_pos_for_postprocessing(spread_pos.pos);
        }
        region.set_block_state(spread_pos.pos, spread_state, UpdateFlags::UPDATE_CLIENTS)
    }

    fn multiface_can_spread_into(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        config: &MultifaceGrowthConfiguration,
        source_pos: BlockPos,
        spread_pos: &MultifaceSpreadPos,
    ) -> bool {
        let existing_state = region.block_state(spread_pos.pos);
        Self::multiface_state_can_be_replaced(
            region,
            registry,
            config,
            source_pos,
            spread_pos.pos,
            spread_pos.face,
            existing_state,
        ) && Self::multiface_is_valid_state_for_placement(
            region,
            registry,
            config,
            existing_state,
            spread_pos.pos,
            spread_pos.face,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Mirrors vanilla MultifaceSpreader"
    )]
    fn multiface_state_can_be_replaced(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        config: &MultifaceGrowthConfiguration,
        source_pos: BlockPos,
        placement_pos: BlockPos,
        placement_direction: Direction,
        existing_state: BlockStateId,
    ) -> bool {
        if Self::multiface_is_sculk_vein(registry, config) {
            return Self::sculk_vein_state_can_be_replaced(
                region,
                registry,
                source_pos,
                placement_pos,
                placement_direction,
                existing_state,
            );
        }

        Self::default_multiface_state_can_be_replaced(registry, config, existing_state)
    }

    fn sculk_vein_state_can_be_replaced(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        source_pos: BlockPos,
        placement_pos: BlockPos,
        placement_direction: Direction,
        existing_state: BlockStateId,
    ) -> bool {
        let against_state = region.block_state(placement_pos.relative(placement_direction));
        if against_state.get_block() == &vanilla_blocks::SCULK
            || against_state.get_block() == &vanilla_blocks::SCULK_CATALYST
            || against_state.get_block() == &vanilla_blocks::MOVING_PISTON
        {
            return false;
        }

        if Self::manhattan_distance(source_pos, placement_pos) == 2 {
            let neighbor_pos = source_pos.relative(placement_direction.opposite());
            if region
                .block_state(neighbor_pos)
                .is_face_sturdy(placement_direction)
            {
                return false;
            }
        }

        let fluid_state = get_fluid_state_from_block(existing_state);
        if !fluid_state.is_empty() && !fluid_state.is_water() {
            return false;
        }

        if registry
            .blocks
            .is_in_tag(existing_state.get_block(), &vanilla_block_tags::FIRE_TAG)
        {
            return false;
        }

        existing_state.is_replaceable()
            || Self::default_multiface_state_can_be_replaced_for_block(
                existing_state,
                &vanilla_blocks::SCULK_VEIN,
            )
    }

    fn default_multiface_state_can_be_replaced(
        registry: &Registry,
        config: &MultifaceGrowthConfiguration,
        existing_state: BlockStateId,
    ) -> bool {
        let place_block = Self::multiface_place_block(registry, config);
        Self::default_multiface_state_can_be_replaced_for_block(existing_state, place_block)
    }

    fn default_multiface_state_can_be_replaced_for_block(
        existing_state: BlockStateId,
        place_block: BlockRef,
    ) -> bool {
        existing_state.is_air()
            || existing_state.get_block() == place_block
            || (existing_state.get_block() == &vanilla_blocks::WATER
                && get_fluid_state_from_block(existing_state).is_source())
    }

    fn multiface_state_for_placement(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        config: &MultifaceGrowthConfiguration,
        old_state: BlockStateId,
        placement_pos: BlockPos,
        placement_direction: Direction,
    ) -> Option<BlockStateId> {
        if !Self::multiface_is_valid_state_for_placement(
            region,
            registry,
            config,
            old_state,
            placement_pos,
            placement_direction,
        ) {
            return None;
        }

        let place_block = Self::multiface_place_block(registry, config);
        let mut new_state = if old_state.get_block() == place_block {
            old_state
        } else {
            let state = registry.blocks.get_default_state_id(place_block);
            let fluid_state = get_fluid_state_from_block(old_state);
            if fluid_state.is_water() && fluid_state.is_source() {
                state.set_value(&BlockStateProperties::WATERLOGGED, true)
            } else {
                state
            }
        };
        new_state = new_state.set_value(Self::multiface_face_property(placement_direction), true);
        Some(new_state)
    }

    fn multiface_is_valid_state_for_placement(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        config: &MultifaceGrowthConfiguration,
        old_state: BlockStateId,
        placement_pos: BlockPos,
        placement_direction: Direction,
    ) -> bool {
        let place_block = Self::multiface_place_block(registry, config);
        if old_state.get_block() == place_block
            && Self::multiface_has_face(old_state, placement_direction)
        {
            return false;
        }

        Self::can_attach_to_multiface(region, placement_pos, placement_direction)
    }

    fn multiface_can_be_placed_on(
        registry: &Registry,
        config: &MultifaceGrowthConfiguration,
        state: BlockStateId,
    ) -> bool {
        config.can_be_placed_on.iter().any(|block_key| {
            let Some(block) = registry.blocks.by_key(block_key) else {
                panic!("multiface growth references unknown block {block_key}");
            };
            state.get_block() == block
        })
    }

    fn multiface_place_block(
        registry: &Registry,
        config: &MultifaceGrowthConfiguration,
    ) -> BlockRef {
        let Some(block) = registry.blocks.by_key(&config.block) else {
            panic!("multiface growth references unknown block {}", config.block);
        };
        block
    }

    fn multiface_is_place_block(
        registry: &Registry,
        config: &MultifaceGrowthConfiguration,
        state: BlockStateId,
    ) -> bool {
        state.get_block() == Self::multiface_place_block(registry, config)
    }

    fn multiface_is_sculk_vein(registry: &Registry, config: &MultifaceGrowthConfiguration) -> bool {
        Self::multiface_place_block(registry, config) == &vanilla_blocks::SCULK_VEIN
    }

    fn multiface_is_other_block_valid_as_source(
        registry: &Registry,
        config: &MultifaceGrowthConfiguration,
        state: BlockStateId,
    ) -> bool {
        Self::multiface_is_sculk_vein(registry, config)
            && state.get_block() != &vanilla_blocks::SCULK_VEIN
    }

    fn multiface_spread_types(_config: &MultifaceGrowthConfiguration) -> [MultifaceSpreadType; 3] {
        [
            MultifaceSpreadType::SamePosition,
            MultifaceSpreadType::SamePlane,
            MultifaceSpreadType::WrapAround,
        ]
    }

    fn multiface_spread_pos(
        pos: BlockPos,
        spread_direction: Direction,
        from_face: Direction,
        spread_type: MultifaceSpreadType,
    ) -> MultifaceSpreadPos {
        match spread_type {
            MultifaceSpreadType::SamePosition => MultifaceSpreadPos {
                pos,
                face: spread_direction,
            },
            MultifaceSpreadType::SamePlane => MultifaceSpreadPos {
                pos: pos.relative(spread_direction),
                face: from_face,
            },
            MultifaceSpreadType::WrapAround => MultifaceSpreadPos {
                pos: pos.relative(spread_direction).relative(from_face),
                face: spread_direction.opposite(),
            },
        }
    }

    fn multiface_shuffled_valid_directions(
        random: &mut WorldgenRandom,
        config: &MultifaceGrowthConfiguration,
    ) -> Vec<Direction> {
        let mut directions = Self::multiface_valid_directions(config);
        Self::shuffle_multiface_directions(random, &mut directions);
        directions
    }

    fn multiface_shuffled_valid_directions_except(
        random: &mut WorldgenRandom,
        config: &MultifaceGrowthConfiguration,
        excluded: Direction,
    ) -> Vec<Direction> {
        let mut directions = Self::multiface_valid_directions(config)
            .into_iter()
            .filter(|direction| *direction != excluded)
            .collect::<Vec<_>>();
        Self::shuffle_multiface_directions(random, &mut directions);
        directions
    }

    fn multiface_valid_directions(config: &MultifaceGrowthConfiguration) -> Vec<Direction> {
        let mut directions = Vec::with_capacity(6);
        if config.can_place_on_ceiling {
            directions.push(Direction::Up);
        }
        if config.can_place_on_floor {
            directions.push(Direction::Down);
        }
        if config.can_place_on_wall {
            directions.extend(Self::VANILLA_HORIZONTAL_DIRECTIONS);
        }
        directions
    }

    fn shuffle_multiface_directions(random: &mut WorldgenRandom, directions: &mut [Direction]) {
        for i in (1..directions.len()).rev() {
            let Ok(bound) = i32::try_from(i + 1) else {
                panic!("multiface direction shuffle length exceeds i32 range");
            };
            let j = random.next_i32_bounded(bound) as usize;
            directions.swap(i, j);
        }
    }

    fn multiface_has_face(state: BlockStateId, direction: Direction) -> bool {
        state
            .try_get_value(Self::multiface_face_property(direction))
            .unwrap_or(false)
    }

    fn multiface_face_property(direction: Direction) -> &'static BoolProperty {
        match direction {
            Direction::Up => &BlockStateProperties::UP,
            Direction::Down => &BlockStateProperties::DOWN,
            Direction::North => &BlockStateProperties::NORTH,
            Direction::South => &BlockStateProperties::SOUTH,
            Direction::East => &BlockStateProperties::EAST,
            Direction::West => &BlockStateProperties::WEST,
        }
    }

    fn multiface_is_air_or_water(state: BlockStateId) -> bool {
        state.is_air() || state.get_block() == &vanilla_blocks::WATER
    }
}
