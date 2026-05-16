use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_netherrack_replace_blobs_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &NetherrackReplaceBlobsConfiguration,
        origin: BlockPos,
    ) -> bool {
        let target = Self::block_state_from_data(registry, &config.target);
        let replacement = Self::block_state_from_data(registry, &config.state);
        let clamped_origin = BlockPos::new(
            origin.x(),
            origin
                .y()
                .clamp(region.min_y() + 1, region.max_y_exclusive() - 1),
            origin.z(),
        );
        let Some(center) = Self::find_replace_blobs_target(region, clamped_origin, target) else {
            return false;
        };

        let radius_x = config.radius.sample(random);
        let radius_y = config.radius.sample(random);
        let radius_z = config.radius.sample(random);
        let maximum_radius = radius_x.max(radius_y).max(radius_z);
        let target_block = target.get_block();
        let mut replaced_any = false;

        Self::for_each_vanilla_within_manhattan(center, radius_x, radius_y, radius_z, |pos| {
            if Self::manhattan_distance(pos, center) > maximum_radius {
                return false;
            }

            if region.block_state(pos).get_block() == target_block {
                let _ = region.set_block_state(pos, replacement, UpdateFlags::UPDATE_CLIENTS);
                replaced_any = true;
            }

            true
        });

        replaced_any
    }

    fn find_replace_blobs_target(
        region: &WorldGenRegion<'_>,
        mut cursor: BlockPos,
        target: BlockStateId,
    ) -> Option<BlockPos> {
        let target_block = target.get_block();
        while cursor.y() > region.min_y() + 1 {
            if region.block_state(cursor).get_block() == target_block {
                return Some(cursor);
            }
            cursor = cursor.below();
        }

        None
    }
}
