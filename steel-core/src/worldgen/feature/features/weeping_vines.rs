use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_weeping_vines_feature(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        if !region.block_state(origin).is_air() {
            return false;
        }

        let above_block = region.block_state(origin.above()).get_block();
        if above_block != &vanilla_blocks::NETHERRACK
            && above_block != &vanilla_blocks::NETHER_WART_BLOCK
        {
            return false;
        }

        Self::place_roof_nether_wart(region, random, origin);
        Self::place_roof_weeping_vines(region, random, origin);
        true
    }

    fn place_roof_nether_wart(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) {
        let wart = vanilla_blocks::NETHER_WART_BLOCK.default_state();
        let _ = region.set_block_state(origin, wart, UpdateFlags::UPDATE_CLIENTS);

        for _ in 0..200 {
            let place_pos = origin.offset(
                random.next_i32_bounded(6) - random.next_i32_bounded(6),
                random.next_i32_bounded(2) - random.next_i32_bounded(5),
                random.next_i32_bounded(6) - random.next_i32_bounded(6),
            );
            if !region.block_state(place_pos).is_air() {
                continue;
            }

            let mut neighbors = 0;
            for direction in Self::VANILLA_DIRECTION_VALUES {
                let neighbor_block = region
                    .block_state(place_pos.relative(direction))
                    .get_block();
                if neighbor_block == &vanilla_blocks::NETHERRACK
                    || neighbor_block == &vanilla_blocks::NETHER_WART_BLOCK
                {
                    neighbors += 1;
                }

                if neighbors > 1 {
                    break;
                }
            }

            if neighbors == 1 {
                let _ = region.set_block_state(place_pos, wart, UpdateFlags::UPDATE_CLIENTS);
            }
        }
    }

    fn place_roof_weeping_vines(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) {
        for _ in 0..100 {
            let place_pos = origin.offset(
                random.next_i32_bounded(8) - random.next_i32_bounded(8),
                random.next_i32_bounded(2) - random.next_i32_bounded(7),
                random.next_i32_bounded(8) - random.next_i32_bounded(8),
            );
            if !region.block_state(place_pos).is_air() {
                continue;
            }

            let above_block = region.block_state(place_pos.above()).get_block();
            if above_block == &vanilla_blocks::NETHERRACK
                || above_block == &vanilla_blocks::NETHER_WART_BLOCK
            {
                let mut vine_height = random.next_i32_between(1, 8);
                if random.next_i32_bounded(6) == 0 {
                    vine_height *= 2;
                }

                if random.next_i32_bounded(5) == 0 {
                    vine_height = 1;
                }

                Self::place_weeping_vines_column(region, random, place_pos, vine_height, 17, 25);
            }
        }
    }

    pub(in crate::worldgen::feature) fn place_weeping_vines_column(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        mut place_pos: BlockPos,
        total_height: i32,
        min_age: u8,
        max_age: u8,
    ) {
        for height in 0..=total_height {
            if region.block_state(place_pos).is_air() {
                if height == total_height || !region.block_state(place_pos.below()).is_air() {
                    let state = vanilla_blocks::WEEPING_VINES.default_state().set_value(
                        &BlockStateProperties::AGE_25,
                        random.next_i32_between(i32::from(min_age), i32::from(max_age)) as u8,
                    );
                    let _ = region.set_block_state(place_pos, state, UpdateFlags::UPDATE_CLIENTS);
                    break;
                }

                let _ = region.set_block_state(
                    place_pos,
                    vanilla_blocks::WEEPING_VINES_PLANT.default_state(),
                    UpdateFlags::UPDATE_CLIENTS,
                );
            }

            place_pos = place_pos.below();
        }
    }
}
