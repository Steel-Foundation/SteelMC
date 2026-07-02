use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_end_island_feature(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        let end_stone = vanilla_blocks::END_STONE.default_state();
        let mut size = random.next_i32_bounded(3) as f32 + 4.0;
        let mut y = 0;

        while size > 0.5 {
            for x in fast_floor(f64::from(-size))..=size.ceil() as i32 {
                for z in fast_floor(f64::from(-size))..=size.ceil() as i32 {
                    if (x * x + z * z) as f32 <= (size + 1.0) * (size + 1.0) {
                        let _ = region.set_block_state(
                            origin.offset(x, y, z),
                            end_stone,
                            UpdateFlags::UPDATE_CLIENTS,
                        );
                    }
                }
            }

            size -= random.next_i32_bounded(2) as f32 + 0.5;
            y -= 1;
        }

        true
    }
}
