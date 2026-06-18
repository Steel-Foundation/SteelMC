use steel_registry::vanilla_block_tags::BlockTag;

use super::super::super::prelude::*;
use super::super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_pointed_dripstone_feature(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        config: &PointedDripstoneConfiguration,
        origin: BlockPos,
    ) -> bool {
        let Some(tip_direction) = Self::pointed_dripstone_tip_direction(region, random, origin)
        else {
            return false;
        };

        let root_pos = origin.relative(tip_direction.opposite());
        Self::create_patch_of_dripstone_blocks(region, random, root_pos, config);
        let height = if random.next_f32() < config.chance_of_taller_dripstone
            && Self::is_empty_or_water(region.block_state(origin.relative(tip_direction)))
        {
            2
        } else {
            1
        };
        Self::grow_pointed_dripstone(region, origin, tip_direction, height, false);
        true
    }

    pub(in crate::worldgen::feature) fn pointed_dripstone_tip_direction(
        region: &WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        pos: BlockPos,
    ) -> Option<Direction> {
        let can_place_above = Self::is_dripstone_base(region.block_state(pos.above()));
        let can_place_below = Self::is_dripstone_base(region.block_state(pos.below()));
        if can_place_above && can_place_below {
            Some(if random.next_bool() {
                Direction::Down
            } else {
                Direction::Up
            })
        } else if can_place_above {
            Some(Direction::Down)
        } else if can_place_below {
            Some(Direction::Up)
        } else {
            None
        }
    }

    fn create_patch_of_dripstone_blocks(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        pos: BlockPos,
        config: &PointedDripstoneConfiguration,
    ) {
        Self::place_dripstone_block_if_possible(region, pos);

        for direction in Self::VANILLA_HORIZONTAL_DIRECTIONS {
            if random.next_f32() > config.chance_of_directional_spread {
                continue;
            }

            let pos1 = pos.relative(direction);
            Self::place_dripstone_block_if_possible(region, pos1);
            if random.next_f32() > config.chance_of_spread_radius2 {
                continue;
            }

            let pos2 = pos1.relative(Self::random_vanilla_direction(random));
            Self::place_dripstone_block_if_possible(region, pos2);
            if random.next_f32() > config.chance_of_spread_radius3 {
                continue;
            }

            let pos3 = pos2.relative(Self::random_vanilla_direction(random));
            Self::place_dripstone_block_if_possible(region, pos3);
        }
    }

    pub(in crate::worldgen::feature) fn grow_pointed_dripstone(
        region: &mut WorldGenRegion<'_>,
        start_pos: BlockPos,
        tip_direction: Direction,
        height: i32,
        merged_tip: bool,
    ) {
        if !Self::is_dripstone_base(
            region.block_state(start_pos.relative(tip_direction.opposite())),
        ) {
            return;
        }

        let mut pos = start_pos;
        Self::build_base_to_tip_dripstone_column(tip_direction, height, merged_tip, |state| {
            let state = if state.get_block() == &vanilla_blocks::POINTED_DRIPSTONE {
                state.set_value(
                    &BlockStateProperties::WATERLOGGED,
                    get_fluid_state_from_block(region.block_state(pos)).is_water(),
                )
            } else {
                state
            };

            let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS);
            pos = pos.relative(tip_direction);
        });
    }

    pub(in crate::worldgen::feature) fn build_base_to_tip_dripstone_column(
        direction: Direction,
        total_length: i32,
        merged_tip: bool,
        mut consumer: impl FnMut(BlockStateId),
    ) {
        if total_length >= 3 {
            consumer(Self::create_pointed_dripstone(
                direction,
                SpeleothemThickness::Base,
            ));

            for _ in 0..total_length - 3 {
                consumer(Self::create_pointed_dripstone(
                    direction,
                    SpeleothemThickness::Middle,
                ));
            }
        }

        if total_length >= 2 {
            consumer(Self::create_pointed_dripstone(
                direction,
                SpeleothemThickness::Frustum,
            ));
        }

        if total_length >= 1 {
            let thickness = if merged_tip {
                SpeleothemThickness::TipMerge
            } else {
                SpeleothemThickness::Tip
            };
            consumer(Self::create_pointed_dripstone(direction, thickness));
        }
    }

    pub(in crate::worldgen::feature) fn place_dripstone_block_if_possible(
        region: &mut WorldGenRegion<'_>,
        pos: BlockPos,
    ) -> bool {
        let state = region.block_state(pos);
        if !state
            .get_block()
            .has_tag(&BlockTag::DRIPSTONE_REPLACEABLE_BLOCKS)
        {
            return false;
        }

        region.set_block_state(
            pos,
            vanilla_blocks::DRIPSTONE_BLOCK.default_state(),
            UpdateFlags::UPDATE_CLIENTS,
        )
    }

    pub(in crate::worldgen::feature) fn create_pointed_dripstone(
        direction: Direction,
        thickness: SpeleothemThickness,
    ) -> BlockStateId {
        vanilla_blocks::POINTED_DRIPSTONE
            .default_state()
            .set_value(&BlockStateProperties::VERTICAL_DIRECTION, direction)
            .set_value(&BlockStateProperties::SPELEOTHEM_THICKNESS, thickness)
    }

    pub(in crate::worldgen::feature) fn is_dripstone_base(state: BlockStateId) -> bool {
        let block = state.get_block();
        block == &vanilla_blocks::DRIPSTONE_BLOCK
            || block.has_tag(&BlockTag::DRIPSTONE_REPLACEABLE_BLOCKS)
    }

    pub(in crate::worldgen::feature) fn is_empty_or_water(state: BlockStateId) -> bool {
        state.is_air() || state.get_block() == &vanilla_blocks::WATER
    }

    fn random_vanilla_direction(random: &mut WorldgenRandom) -> Direction {
        Self::VANILLA_DIRECTION_VALUES[random.next_i32_bounded(6) as usize]
    }
}
