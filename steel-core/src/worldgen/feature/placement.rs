use super::prelude::*;
use super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(super) const fn feature_heightmap_type(heightmap: FeatureHeightmap) -> HeightmapType {
        match heightmap {
            FeatureHeightmap::WorldSurface => HeightmapType::WorldSurface,
            FeatureHeightmap::MotionBlocking => HeightmapType::MotionBlocking,
            FeatureHeightmap::MotionBlockingNoLeaves => HeightmapType::MotionBlockingNoLeaves,
            FeatureHeightmap::OceanFloor => HeightmapType::OceanFloor,
            FeatureHeightmap::WorldSurfaceWg => HeightmapType::WorldSurfaceWg,
            FeatureHeightmap::OceanFloorWg => HeightmapType::OceanFloorWg,
        }
    }

    pub(super) fn count_on_every_layer_positions(
        region: &WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        origin: BlockPos,
        count: &IntProvider,
    ) -> Vec<BlockPos> {
        let mut positions = Vec::new();
        let mut layer = 0;

        loop {
            let mut found_any = false;
            for _ in 0..count.sample(random) {
                let x = origin.x() + random.next_i32_bounded(16);
                let z = origin.z() + random.next_i32_bounded(16);
                let start_y = region.height_at(HeightmapType::MotionBlocking, x, z);
                if let Some(y) = Self::find_on_ground_y_position(region, x, start_y, z, layer) {
                    positions.push(BlockPos::new(x, y, z));
                    found_any = true;
                }
            }

            if !found_any {
                break;
            }
            layer += 1;
        }

        positions
    }

    pub(super) fn find_on_ground_y_position(
        region: &WorldGenRegion<'_>,
        x: i32,
        start_y: i32,
        z: i32,
        layer_to_place_on: i32,
    ) -> Option<i32> {
        let mut current_layer = 0;
        let mut current_block = region.block_state(BlockPos::new(x, start_y, z));

        for y in (region.min_y() + 1..=start_y).rev() {
            let below_block = region.block_state(BlockPos::new(x, y - 1, z));
            if !Self::is_empty_layer_block(below_block)
                && Self::is_empty_layer_block(current_block)
                && below_block.get_block() != &vanilla_blocks::BEDROCK
            {
                if current_layer == layer_to_place_on {
                    return Some(y);
                }
                current_layer += 1;
            }

            current_block = below_block;
        }

        None
    }

    pub(super) fn is_empty_layer_block(state: steel_utils::BlockStateId) -> bool {
        state.is_air()
            || state.get_block() == &vanilla_blocks::WATER
            || state.get_block() == &vanilla_blocks::LAVA
    }

    pub(super) fn environment_scan_position(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        origin: BlockPos,
        direction_of_search: steel_utils::Direction,
        target_condition: &BlockPredicate,
        allowed_search_condition: Option<&BlockPredicate>,
        max_steps: i32,
    ) -> Option<BlockPos> {
        assert!(
            max_steps > 0,
            "environment scan max_steps must be positive, got {max_steps}"
        );

        let mut position = origin;
        if !Self::test_optional_block_predicate(
            region,
            registry,
            allowed_search_condition,
            position,
        ) {
            return None;
        }

        for _ in 0..max_steps {
            if Self::test_block_predicate(region, registry, target_condition, position) {
                return Some(position);
            }

            position = position.relative(direction_of_search);
            if region.is_outside_build_height(position.y()) {
                return None;
            }

            if !Self::test_optional_block_predicate(
                region,
                registry,
                allowed_search_condition,
                position,
            ) {
                break;
            }
        }

        if Self::test_block_predicate(region, registry, target_condition, position) {
            Some(position)
        } else {
            None
        }
    }
}
