use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_glowstone_blob_feature(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        if !region.block_state(origin).is_air() {
            return false;
        }

        let above_block = region.block_state(origin.above()).get_block();
        if above_block != &vanilla_blocks::NETHERRACK
            && above_block != &vanilla_blocks::BASALT
            && above_block != &vanilla_blocks::BLACKSTONE
        {
            return false;
        }

        let glowstone = vanilla_blocks::GLOWSTONE.default_state();
        let _ = region.set_block_state(origin, glowstone, UpdateFlags::UPDATE_CLIENTS);

        for _ in 0..1500 {
            let place_pos = origin.offset(
                random.next_i32_bounded(8) - random.next_i32_bounded(8),
                -random.next_i32_bounded(12),
                random.next_i32_bounded(8) - random.next_i32_bounded(8),
            );
            if !region.block_state(place_pos).is_air() {
                continue;
            }

            let mut neighbors = 0;
            for direction in Self::VANILLA_DIRECTION_VALUES {
                if region
                    .block_state(place_pos.relative(direction))
                    .get_block()
                    == &vanilla_blocks::GLOWSTONE
                {
                    neighbors += 1;
                }

                if neighbors > 1 {
                    break;
                }
            }

            if neighbors == 1 {
                let _ = region.set_block_state(place_pos, glowstone, UpdateFlags::UPDATE_CLIENTS);
            }
        }

        true
    }
}
