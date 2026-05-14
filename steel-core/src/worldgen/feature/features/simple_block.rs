use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_simple_block_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &SimpleBlockConfiguration,
        origin: BlockPos,
    ) -> bool {
        let Some(state_to_place) = Self::sample_block_state_provider_optional(
            region,
            registry,
            random,
            &config.to_place,
            origin,
        ) else {
            return false;
        };

        let behavior = BLOCK_BEHAVIORS.get_behavior(state_to_place.get_block());
        if !behavior.can_survive(state_to_place, region, origin) {
            return false;
        }

        if Self::is_double_plant_block(state_to_place.get_block()) {
            if !region.block_state(origin.above()).is_air() {
                return false;
            }
            Self::place_double_plant(region, state_to_place, origin);
        } else if state_to_place.get_block() == &vanilla_blocks::PALE_MOSS_CARPET {
            Self::place_mossy_carpet(region, random, origin);
        } else {
            let _ = region.set_block_state(origin, state_to_place, UpdateFlags::UPDATE_CLIENTS);
        }

        if config.schedule_tick {
            let placed_state = region.block_state(origin);
            let _ = region.schedule_block_tick_default(origin, placed_state.get_block(), 1);
        }

        true
    }

    pub(in crate::worldgen::feature) fn is_double_plant_block(block: BlockRef) -> bool {
        block == &vanilla_blocks::SUNFLOWER
            || block == &vanilla_blocks::LILAC
            || block == &vanilla_blocks::ROSE_BUSH
            || block == &vanilla_blocks::PEONY
            || block == &vanilla_blocks::TALL_GRASS
            || block == &vanilla_blocks::LARGE_FERN
            || block == &vanilla_blocks::PITCHER_PLANT
            || block == &vanilla_blocks::SMALL_DRIPLEAF
    }

    pub(in crate::worldgen::feature) fn place_double_plant(
        region: &mut WorldGenRegion<'_>,
        state: BlockStateId,
        lower_pos: BlockPos,
    ) {
        let upper_pos = lower_pos.above();
        let lower_state = Self::copy_waterlogged_from(
            region,
            lower_pos,
            state.set_value(
                &BlockStateProperties::DOUBLE_BLOCK_HALF,
                DoubleBlockHalf::Lower,
            ),
        );
        let upper_state = Self::copy_waterlogged_from(
            region,
            upper_pos,
            state.set_value(
                &BlockStateProperties::DOUBLE_BLOCK_HALF,
                DoubleBlockHalf::Upper,
            ),
        );
        let _ = region.set_block_state(lower_pos, lower_state, UpdateFlags::UPDATE_CLIENTS);
        let _ = region.set_block_state(upper_pos, upper_state, UpdateFlags::UPDATE_CLIENTS);
    }

    pub(in crate::worldgen::feature) fn copy_waterlogged_from(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockStateId {
        if state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .is_none()
        {
            return state;
        }

        let waterlogged = get_fluid_state_from_block(region.block_state(pos)).is_water();
        state.set_value(&BlockStateProperties::WATERLOGGED, waterlogged)
    }

    pub(in crate::worldgen::feature) fn place_mossy_carpet(
        region: &mut WorldGenRegion<'_>,
        random: &mut Xoroshiro,
        pos: BlockPos,
    ) {
        let simple_carpet_layer = vanilla_blocks::PALE_MOSS_CARPET.default_state();
        let adjusted_carpet_layer =
            Self::updated_mossy_carpet_state(region, simple_carpet_layer, pos, true);
        let _ = region.set_block_state(pos, adjusted_carpet_layer, UpdateFlags::UPDATE_CLIENTS);

        let topper = Self::create_mossy_carpet_topper(region, random, pos);
        if !topper.is_air() {
            let _ = region.set_block_state(pos.above(), topper, UpdateFlags::UPDATE_CLIENTS);
            let update_bottom =
                Self::updated_mossy_carpet_state(region, adjusted_carpet_layer, pos, true);
            let _ = region.set_block_state(pos, update_bottom, UpdateFlags::UPDATE_CLIENTS);
        }
    }

    pub(in crate::worldgen::feature) fn create_mossy_carpet_topper(
        region: &WorldGenRegion<'_>,
        random: &mut Xoroshiro,
        pos: BlockPos,
    ) -> BlockStateId {
        let above = pos.above();
        let above_previous_state = region.block_state(above);
        let is_mossy_carpet_above =
            above_previous_state.get_block() == &vanilla_blocks::PALE_MOSS_CARPET;
        if (!is_mossy_carpet_above
            || !above_previous_state.get_value(&BlockStateProperties::BOTTOM))
            && (is_mossy_carpet_above || above_previous_state.is_replaceable())
        {
            let no_base_state = vanilla_blocks::PALE_MOSS_CARPET
                .default_state()
                .set_value(&BlockStateProperties::BOTTOM, false);
            let mut above_state =
                Self::updated_mossy_carpet_state(region, no_base_state, above, true);

            for direction in Self::HORIZONTAL_DIRECTIONS {
                let property = Self::mossy_carpet_wall_property(direction);
                if above_state.get_value(&property) != WallSide::None && !random.next_bool() {
                    above_state = above_state.set_value(&property, WallSide::None);
                }
            }

            if Self::mossy_carpet_has_faces(above_state) && above_state != above_previous_state {
                above_state
            } else {
                vanilla_blocks::AIR.default_state()
            }
        } else {
            vanilla_blocks::AIR.default_state()
        }
    }

    pub(in crate::worldgen::feature) fn updated_mossy_carpet_state(
        region: &WorldGenRegion<'_>,
        mut state: BlockStateId,
        pos: BlockPos,
        create_sides: bool,
    ) -> BlockStateId {
        let create_sides = create_sides || state.get_value(&BlockStateProperties::BOTTOM);

        for direction in Self::HORIZONTAL_DIRECTIONS {
            let property = Self::mossy_carpet_wall_property(direction);
            let mut side = if Self::mossy_carpet_can_support_at_face(region, pos, direction) {
                if create_sides {
                    WallSide::Low
                } else {
                    state.get_value(&property)
                }
            } else {
                WallSide::None
            };

            if side == WallSide::Low {
                let above_state = region.block_state(pos.above());
                if above_state.get_block() == &vanilla_blocks::PALE_MOSS_CARPET
                    && above_state.get_value(&property) != WallSide::None
                    && !above_state.get_value(&BlockStateProperties::BOTTOM)
                {
                    side = WallSide::Tall;
                }

                if !state.get_value(&BlockStateProperties::BOTTOM) {
                    let below_state = region.block_state(pos.below());
                    if below_state.get_block() == &vanilla_blocks::PALE_MOSS_CARPET
                        && below_state.get_value(&property) == WallSide::None
                    {
                        side = WallSide::None;
                    }
                }
            }

            state = state.set_value(&property, side);
        }

        state
    }

    const HORIZONTAL_DIRECTIONS: [Direction; 4] = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];

    pub(in crate::worldgen::feature) fn mossy_carpet_wall_property(
        direction: Direction,
    ) -> EnumProperty<WallSide> {
        match direction {
            Direction::North => BlockStateProperties::NORTH_WALL,
            Direction::East => BlockStateProperties::EAST_WALL,
            Direction::South => BlockStateProperties::SOUTH_WALL,
            Direction::West => BlockStateProperties::WEST_WALL,
            Direction::Down | Direction::Up => {
                panic!("mossy carpet has no wall property for vertical direction")
            }
        }
    }

    pub(in crate::worldgen::feature) fn mossy_carpet_can_support_at_face(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        direction: Direction,
    ) -> bool {
        direction != Direction::Up && Self::can_attach_to_multiface(region, pos, direction)
    }

    pub(in crate::worldgen::feature) fn can_attach_to_multiface(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        direction_towards_neighbor: Direction,
    ) -> bool {
        let neighbor_pos = pos.relative(direction_towards_neighbor);
        let neighbor_state = region.block_state(neighbor_pos);
        let support_direction = direction_towards_neighbor.opposite();
        shapes::is_face_full(neighbor_state.get_support_shape(), support_direction)
            || shapes::is_face_full(neighbor_state.get_collision_shape(), support_direction)
    }

    pub(in crate::worldgen::feature) fn mossy_carpet_has_faces(state: BlockStateId) -> bool {
        if state.get_value(&BlockStateProperties::BOTTOM) {
            return true;
        }

        for direction in Self::HORIZONTAL_DIRECTIONS {
            let property = Self::mossy_carpet_wall_property(direction);
            if state.get_value(&property) != WallSide::None {
                return true;
            }
        }

        false
    }
}
