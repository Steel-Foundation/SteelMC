use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_block_blob_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &BlockBlobConfiguration,
        mut origin: BlockPos,
    ) -> bool {
        while origin.y() > region.min_y() + 3
            && !Self::test_block_predicate(region, registry, &config.can_place_on, origin.below())
        {
            origin = origin.below();
        }

        if origin.y() <= region.min_y() + 3 {
            return false;
        }

        let state = Self::block_state_from_data(registry, &config.state);
        for _ in 0..3 {
            let x_radius = random.next_i32_bounded(2);
            let y_radius = random.next_i32_bounded(2);
            let z_radius = random.next_i32_bounded(2);
            let threshold = (x_radius + y_radius + z_radius) as f32 * 0.333 + 0.5;
            let threshold_squared = threshold * threshold;

            for x in origin.x() - x_radius..=origin.x() + x_radius {
                for y in origin.y() - y_radius..=origin.y() + y_radius {
                    for z in origin.z() - z_radius..=origin.z() + z_radius {
                        let dx = x - origin.x();
                        let dy = y - origin.y();
                        let dz = z - origin.z();
                        if (dx * dx + dy * dy + dz * dz) as f32 <= threshold_squared {
                            let _ = region.set_block_state(
                                BlockPos::new(x, y, z),
                                state,
                                UpdateFlags::UPDATE_ALL,
                            );
                        }
                    }
                }
            }

            origin = origin.offset(
                -1 + random.next_i32_bounded(2),
                -random.next_i32_bounded(2),
                -1 + random.next_i32_bounded(2),
            );
        }

        true
    }
}
