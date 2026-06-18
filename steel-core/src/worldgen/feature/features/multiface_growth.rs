use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;
use smallvec::SmallVec;
use steel_registry::vanilla_block_tags::BlockTag;

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

struct ResolvedMultifaceGrowth<'a> {
    raw: &'a MultifaceGrowthConfiguration,
    place_block: BlockRef,
    default_state: BlockStateId,
    can_be_placed_on: &'a [BlockRef],
    is_sculk_vein: bool,
}

type MultifaceDirections = SmallVec<[Direction; 6]>;

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

        let resolved_config = Self::resolve_multiface_growth_config(registry, config);
        let search_directions = Self::multiface_shuffled_valid_directions(random, &resolved_config);
        if Self::place_multiface_growth_if_possible(
            region,
            random,
            origin,
            origin_state,
            &resolved_config,
            &search_directions,
        ) {
            return true;
        }

        for search_direction in &search_directions {
            let placement_directions = Self::multiface_shuffled_valid_directions_except(
                random,
                &resolved_config,
                search_direction.opposite(),
            );

            for _ in 0..resolved_config.raw.search_range {
                let pos = origin.relative(*search_direction);
                let state = region.block_state(pos);
                if !Self::multiface_is_air_or_water(state)
                    && !Self::multiface_is_place_block(&resolved_config, state)
                {
                    break;
                }

                if Self::place_multiface_growth_if_possible(
                    region,
                    random,
                    pos,
                    state,
                    &resolved_config,
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
        random: &mut WorldgenRandom,
        pos: BlockPos,
        old_state: BlockStateId,
        config: &ResolvedMultifaceGrowth<'_>,
        placement_directions: &[Direction],
    ) -> bool {
        for placement_direction in placement_directions {
            let neighbor_state = region.block_state(pos.relative(*placement_direction));
            if !Self::multiface_can_be_placed_on(config, neighbor_state) {
                continue;
            }

            let Some(new_state) = Self::multiface_state_for_placement(
                region,
                config,
                old_state,
                pos,
                *placement_direction,
            ) else {
                return false;
            };

            let _ = region.set_block_state(pos, new_state, UpdateFlags::UPDATE_ALL);
            region.mark_pos_for_postprocessing(pos);
            if random.next_f32() < config.raw.chance_of_spreading {
                let _ = Self::spread_multiface_from_face_toward_random_direction(
                    region,
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

        random: &mut WorldgenRandom,
        config: &ResolvedMultifaceGrowth<'_>,
        state: BlockStateId,
        pos: BlockPos,
        starting_face: Direction,
        post_process: bool,
    ) -> Option<MultifaceSpreadPos> {
        let directions = Self::shuffled_directions(random, Self::VANILLA_DIRECTION_VALUES);
        for spread_direction in directions {
            if let Some(spread_pos) = Self::spread_multiface_from_face_toward_direction(
                region,
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

    fn spread_multiface_from_face_toward_direction(
        region: &mut WorldGenRegion<'_>,
        config: &ResolvedMultifaceGrowth<'_>,
        state: BlockStateId,
        pos: BlockPos,
        starting_face: Direction,
        spread_direction: Direction,
        post_process: bool,
    ) -> Option<MultifaceSpreadPos> {
        let spread_pos = Self::multiface_spread_from_face_toward_direction(
            region,
            config,
            state,
            pos,
            starting_face,
            spread_direction,
        )?;
        if Self::spread_multiface_to_face(region, config, &spread_pos, post_process) {
            Some(spread_pos)
        } else {
            None
        }
    }

    fn multiface_spread_from_face_toward_direction(
        region: &WorldGenRegion<'_>,
        config: &ResolvedMultifaceGrowth<'_>,
        state: BlockStateId,
        pos: BlockPos,
        starting_face: Direction,
        spread_direction: Direction,
    ) -> Option<MultifaceSpreadPos> {
        if spread_direction.axis() == starting_face.axis() {
            return None;
        }

        if !Self::multiface_is_other_block_valid_as_source(config, state)
            && (!Self::multiface_has_face(state, starting_face)
                || Self::multiface_has_face(state, spread_direction))
        {
            return None;
        }

        for spread_type in Self::multiface_spread_types(config) {
            let spread_pos =
                Self::multiface_spread_pos(pos, spread_direction, starting_face, spread_type);
            if Self::multiface_can_spread_into(region, config, pos, &spread_pos) {
                return Some(spread_pos);
            }
        }

        None
    }

    fn spread_multiface_to_face(
        region: &mut WorldGenRegion<'_>,
        config: &ResolvedMultifaceGrowth<'_>,
        spread_pos: &MultifaceSpreadPos,
        post_process: bool,
    ) -> bool {
        let old_state = region.block_state(spread_pos.pos);
        let Some(spread_state) = Self::multiface_state_for_placement(
            region,
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

        config: &ResolvedMultifaceGrowth<'_>,
        source_pos: BlockPos,
        spread_pos: &MultifaceSpreadPos,
    ) -> bool {
        let existing_state = region.block_state(spread_pos.pos);
        Self::multiface_state_can_be_replaced(
            region,
            config,
            source_pos,
            spread_pos.pos,
            spread_pos.face,
            existing_state,
        ) && Self::multiface_is_valid_state_for_placement(
            region,
            config,
            existing_state,
            spread_pos.pos,
            spread_pos.face,
        )
    }

    fn multiface_state_can_be_replaced(
        region: &WorldGenRegion<'_>,

        config: &ResolvedMultifaceGrowth<'_>,
        source_pos: BlockPos,
        placement_pos: BlockPos,
        placement_direction: Direction,
        existing_state: BlockStateId,
    ) -> bool {
        if config.is_sculk_vein {
            return Self::sculk_vein_state_can_be_replaced(
                region,
                source_pos,
                placement_pos,
                placement_direction,
                existing_state,
            );
        }

        Self::default_multiface_state_can_be_replaced(config, existing_state)
    }

    fn sculk_vein_state_can_be_replaced(
        region: &WorldGenRegion<'_>,
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
                .is_face_sturdy_at(neighbor_pos, placement_direction)
            {
                return false;
            }
        }

        let fluid_state = get_fluid_state_from_block(existing_state);
        if !fluid_state.is_empty() && !fluid_state.is_water() {
            return false;
        }

        if existing_state.get_block().has_tag(&BlockTag::FIRE) {
            return false;
        }

        existing_state.is_replaceable()
            || Self::default_multiface_state_can_be_replaced_for_block(
                existing_state,
                &vanilla_blocks::SCULK_VEIN,
            )
    }

    fn default_multiface_state_can_be_replaced(
        config: &ResolvedMultifaceGrowth<'_>,
        existing_state: BlockStateId,
    ) -> bool {
        Self::default_multiface_state_can_be_replaced_for_block(existing_state, config.place_block)
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
        config: &ResolvedMultifaceGrowth<'_>,
        old_state: BlockStateId,
        placement_pos: BlockPos,
        placement_direction: Direction,
    ) -> Option<BlockStateId> {
        if !Self::multiface_is_valid_state_for_placement(
            region,
            config,
            old_state,
            placement_pos,
            placement_direction,
        ) {
            return None;
        }

        let mut new_state = if old_state.get_block() == config.place_block {
            old_state
        } else {
            let fluid_state = get_fluid_state_from_block(old_state);
            if fluid_state.is_water() && fluid_state.is_source() {
                config
                    .default_state
                    .set_value(&BlockStateProperties::WATERLOGGED, true)
            } else {
                config.default_state
            }
        };
        new_state = new_state.set_value(Self::multiface_face_property(placement_direction), true);
        Some(new_state)
    }

    fn multiface_is_valid_state_for_placement(
        region: &WorldGenRegion<'_>,
        config: &ResolvedMultifaceGrowth<'_>,
        old_state: BlockStateId,
        placement_pos: BlockPos,
        placement_direction: Direction,
    ) -> bool {
        if old_state.get_block() == config.place_block
            && Self::multiface_has_face(old_state, placement_direction)
        {
            return false;
        }

        Self::can_attach_to_multiface(region, placement_pos, placement_direction)
    }

    fn resolve_multiface_growth_config<'a>(
        registry: &Registry,
        config: &'a MultifaceGrowthConfiguration,
    ) -> ResolvedMultifaceGrowth<'a> {
        ResolvedMultifaceGrowth {
            raw: config,
            place_block: config.block,
            default_state: registry.blocks.get_default_state_id(config.block),
            can_be_placed_on: &config.can_be_placed_on,
            is_sculk_vein: config.block == &vanilla_blocks::SCULK_VEIN,
        }
    }

    fn multiface_can_be_placed_on(
        config: &ResolvedMultifaceGrowth<'_>,
        state: BlockStateId,
    ) -> bool {
        config
            .can_be_placed_on
            .iter()
            .any(|block| state.get_block() == *block)
    }

    fn multiface_is_place_block(config: &ResolvedMultifaceGrowth<'_>, state: BlockStateId) -> bool {
        state.get_block() == config.place_block
    }

    fn multiface_is_other_block_valid_as_source(
        config: &ResolvedMultifaceGrowth<'_>,
        state: BlockStateId,
    ) -> bool {
        config.is_sculk_vein && state.get_block() != &vanilla_blocks::SCULK_VEIN
    }

    const fn multiface_spread_types(
        _config: &ResolvedMultifaceGrowth<'_>,
    ) -> [MultifaceSpreadType; 3] {
        [
            MultifaceSpreadType::SamePosition,
            MultifaceSpreadType::SamePlane,
            MultifaceSpreadType::WrapAround,
        ]
    }

    const fn multiface_spread_pos(
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
        config: &ResolvedMultifaceGrowth<'_>,
    ) -> MultifaceDirections {
        let mut directions = Self::multiface_valid_directions(config);
        Self::shuffle_multiface_directions(random, &mut directions);
        directions
    }

    fn multiface_shuffled_valid_directions_except(
        random: &mut WorldgenRandom,
        config: &ResolvedMultifaceGrowth<'_>,
        excluded: Direction,
    ) -> MultifaceDirections {
        let mut directions = Self::multiface_valid_directions(config);
        directions.retain(|direction| *direction != excluded);
        Self::shuffle_multiface_directions(random, &mut directions);
        directions
    }

    fn multiface_valid_directions(config: &ResolvedMultifaceGrowth<'_>) -> MultifaceDirections {
        let mut directions = MultifaceDirections::new();
        if config.raw.can_place_on_ceiling {
            directions.push(Direction::Up);
        }
        if config.raw.can_place_on_floor {
            directions.push(Direction::Down);
        }
        if config.raw.can_place_on_wall {
            directions.extend_from_slice(&Self::VANILLA_HORIZONTAL_DIRECTIONS);
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

    const fn multiface_face_property(direction: Direction) -> &'static BoolProperty {
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
