use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;
use crate::behavior::blocks::MossyCarpetBlock;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_simple_block_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
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
            Self::place_mossy_carpet(region, origin);
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
        pos: BlockPos,
    ) {
        let simple_carpet_layer = vanilla_blocks::PALE_MOSS_CARPET.default_state();
        let adjusted_carpet_layer =
            MossyCarpetBlock::updated_state(region, simple_carpet_layer, pos, true);
        let _ = region.set_block_state(pos, adjusted_carpet_layer, UpdateFlags::UPDATE_CLIENTS);

        let topper = Self::create_mossy_carpet_topper(region, pos);
        if !topper.is_air() {
            let _ = region.set_block_state(pos.above(), topper, UpdateFlags::UPDATE_CLIENTS);
            let update_bottom =
                MossyCarpetBlock::updated_state(region, adjusted_carpet_layer, pos, true);
            let _ = region.set_block_state(pos, update_bottom, UpdateFlags::UPDATE_CLIENTS);
        }
    }

    pub(in crate::worldgen::feature) fn create_mossy_carpet_topper(
        region: &mut WorldGenRegion<'_>,
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
                MossyCarpetBlock::updated_state(region, no_base_state, above, true);

            for direction in MossyCarpetBlock::HORIZONTAL_DIRECTIONS {
                let property = MossyCarpetBlock::wall_property(direction);
                if above_state.get_value(&property) != WallSide::None
                    && !region.random_mut().next_bool()
                {
                    above_state = above_state.set_value(&property, WallSide::None);
                }
            }

            if MossyCarpetBlock::has_faces(above_state) && above_state != above_previous_state {
                above_state
            } else {
                vanilla_blocks::AIR.default_state()
            }
        } else {
            vanilla_blocks::AIR.default_state()
        }
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
}
