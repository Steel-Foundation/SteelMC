use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

const CLUSTERED_REACH: i32 = 5;
const CLUSTERED_SIZE: i32 = 50;
const UNCLUSTERED_REACH: i32 = 8;
const UNCLUSTERED_SIZE: i32 = 15;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_basalt_columns_feature(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        config: &BasaltColumnsConfiguration,
        origin: BlockPos,
    ) -> bool {
        let lava_sea_level = region.sea_level();
        if !Self::can_place_basalt_column_at(region, lava_sea_level, origin) {
            return false;
        }

        let column_height = config.height.sample(random);
        let generate_clustered = random.next_f32() < 0.9;
        let reach = column_height.min(if generate_clustered {
            CLUSTERED_REACH
        } else {
            UNCLUSTERED_REACH
        });
        let count = if generate_clustered {
            CLUSTERED_SIZE
        } else {
            UNCLUSTERED_SIZE
        };
        let mut placed = false;

        for _ in 0..count {
            let pos = BlockPos::new(
                origin.x() - reach + random.next_i32_bounded(reach * 2 + 1),
                origin.y() + random.next_i32_bounded(1),
                origin.z() - reach + random.next_i32_bounded(reach * 2 + 1),
            );
            let blocks_to_place_y = column_height - Self::manhattan_distance(pos, origin);
            if blocks_to_place_y >= 0 {
                placed |= Self::place_basalt_column(
                    region,
                    lava_sea_level,
                    pos,
                    blocks_to_place_y,
                    config.reach.sample(random),
                );
            }
        }

        placed
    }

    fn place_basalt_column(
        region: &mut WorldGenRegion<'_>,
        lava_sea_level: i32,
        origin: BlockPos,
        column_height: i32,
        reach: i32,
    ) -> bool {
        let mut placed_any = false;

        for z in origin.z() - reach..=origin.z() + reach {
            for x in origin.x() - reach..=origin.x() + reach {
                let pos = BlockPos::new(x, origin.y(), z);
                let step_limit = Self::manhattan_distance(pos, origin);
                let column_pos = if Self::is_air_or_lava_ocean(region, lava_sea_level, pos) {
                    Self::find_basalt_column_surface(region, lava_sea_level, pos, step_limit)
                } else {
                    Self::find_basalt_column_air(region, pos, step_limit)
                };

                let Some(mut cursor) = column_pos else {
                    continue;
                };

                let mut blocks_y = column_height - step_limit / 2;
                while blocks_y >= 0 {
                    if Self::is_air_or_lava_ocean(region, lava_sea_level, cursor) {
                        let _ = region.set_block_state(
                            cursor,
                            vanilla_blocks::BASALT.default_state(),
                            UpdateFlags::UPDATE_CLIENTS,
                        );
                        placed_any = true;
                        cursor = cursor.above();
                    } else {
                        if region.block_state(cursor).get_block() != &vanilla_blocks::BASALT {
                            break;
                        }
                        cursor = cursor.above();
                    }

                    blocks_y -= 1;
                }
            }
        }

        placed_any
    }

    fn find_basalt_column_surface(
        region: &WorldGenRegion<'_>,
        lava_sea_level: i32,
        mut cursor: BlockPos,
        mut limit: i32,
    ) -> Option<BlockPos> {
        while cursor.y() > region.min_y() + 1 && limit > 0 {
            limit -= 1;
            if Self::can_place_basalt_column_at(region, lava_sea_level, cursor) {
                return Some(cursor);
            }
            cursor = cursor.below();
        }

        None
    }

    fn find_basalt_column_air(
        region: &WorldGenRegion<'_>,
        mut cursor: BlockPos,
        mut limit: i32,
    ) -> Option<BlockPos> {
        while cursor.y() < region.max_y_exclusive() && limit > 0 {
            limit -= 1;
            let state = region.block_state(cursor);
            if Self::basalt_columns_cannot_place_on(state.get_block()) {
                return None;
            }

            if state.is_air() {
                return Some(cursor);
            }

            cursor = cursor.above();
        }

        None
    }

    fn can_place_basalt_column_at(
        region: &WorldGenRegion<'_>,
        lava_sea_level: i32,
        pos: BlockPos,
    ) -> bool {
        if !Self::is_air_or_lava_ocean(region, lava_sea_level, pos) {
            return false;
        }

        let below = region.block_state(pos.below());
        !below.is_air() && !Self::basalt_columns_cannot_place_on(below.get_block())
    }

    fn is_air_or_lava_ocean(
        region: &WorldGenRegion<'_>,
        lava_sea_level: i32,
        pos: BlockPos,
    ) -> bool {
        let state = region.block_state(pos);
        state.is_air() || state.get_block() == &vanilla_blocks::LAVA && pos.y() <= lava_sea_level
    }

    fn basalt_columns_cannot_place_on(block: BlockRef) -> bool {
        block == &vanilla_blocks::LAVA
            || block == &vanilla_blocks::BEDROCK
            || block == &vanilla_blocks::MAGMA_BLOCK
            || block == &vanilla_blocks::SOUL_SAND
            || block == &vanilla_blocks::NETHER_BRICKS
            || block == &vanilla_blocks::NETHER_BRICK_FENCE
            || block == &vanilla_blocks::NETHER_BRICK_STAIRS
            || block == &vanilla_blocks::NETHER_WART
            || block == &vanilla_blocks::CHEST
            || block == &vanilla_blocks::SPAWNER
    }
}
