use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_delta_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &DeltaFeatureConfiguration,
        origin: BlockPos,
    ) -> bool {
        let contents = Self::block_state_from_data(registry, &config.contents);
        let rim = Self::block_state_from_data(registry, &config.rim);
        let spawn_rim = random.next_f64() < 0.9;
        let rim_x = if spawn_rim {
            config.rim_size.sample(random)
        } else {
            0
        };
        let rim_z = if spawn_rim {
            config.rim_size.sample(random)
        } else {
            0
        };
        let has_rim = spawn_rim && rim_x != 0 && rim_z != 0;
        let radius_x = config.size.sample(random);
        let radius_z = config.size.sample(random);
        let radius_limit = radius_x.max(radius_z);
        let mut any_placed = false;

        Self::for_each_vanilla_within_manhattan(origin, radius_x, 0, radius_z, |pos| {
            if Self::manhattan_distance(pos, origin) > radius_limit {
                return false;
            }

            if Self::delta_is_clear(region, pos, contents) {
                if has_rim {
                    let _ = region.set_block_state(pos, rim, UpdateFlags::UPDATE_CLIENTS);
                    any_placed = true;
                }

                let offset_pos = pos.offset(rim_x, 0, rim_z);
                if Self::delta_is_clear(region, offset_pos, contents) {
                    let _ =
                        region.set_block_state(offset_pos, contents, UpdateFlags::UPDATE_CLIENTS);
                    any_placed = true;
                }
            }

            true
        });

        any_placed
    }

    fn delta_is_clear(region: &WorldGenRegion<'_>, pos: BlockPos, contents: BlockStateId) -> bool {
        let state = region.block_state(pos);
        if state.get_block() == contents.get_block() {
            return false;
        }

        if Self::delta_cannot_replace(state.get_block()) {
            return false;
        }

        for direction in Self::VANILLA_DIRECTION_VALUES {
            let is_air = region.block_state(pos.relative(direction)).is_air();
            if (is_air && direction != Direction::Up) || (!is_air && direction == Direction::Up) {
                return false;
            }
        }

        true
    }

    fn delta_cannot_replace(block: BlockRef) -> bool {
        block == &vanilla_blocks::BEDROCK
            || block == &vanilla_blocks::NETHER_BRICKS
            || block == &vanilla_blocks::NETHER_BRICK_FENCE
            || block == &vanilla_blocks::NETHER_BRICK_STAIRS
            || block == &vanilla_blocks::NETHER_WART
            || block == &vanilla_blocks::CHEST
            || block == &vanilla_blocks::SPAWNER
    }
}
