use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_stepped_column_cluster_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &SteppedColumnClusterConfiguration,
        origin: BlockPos,
    ) -> bool {
        if !Self::can_place_stepped_column_at(region, registry, config, origin) {
            return false;
        }

        let column_height = config.height.sample(random);
        let cluster_reach = column_height.min(config.cluster_reach.sample(random));
        let column_count = config.column_count.sample(random);
        let mut placed = false;

        for _ in 0..column_count {
            let pos = BlockPos::new(
                origin.x() - cluster_reach + random.next_i32_bounded(cluster_reach * 2 + 1),
                origin.y() + random.next_i32_bounded(1),
                origin.z() - cluster_reach + random.next_i32_bounded(cluster_reach * 2 + 1),
            );
            let blocks_to_place = column_height - Self::manhattan_distance(pos, origin);
            if blocks_to_place >= 0 {
                let column_reach = config.column_reach.sample(random);
                placed |= Self::place_stepped_column(
                    region,
                    registry,
                    random,
                    config,
                    pos,
                    blocks_to_place,
                    column_reach,
                );
            }
        }

        placed
    }

    fn place_stepped_column(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &SteppedColumnClusterConfiguration,
        origin: BlockPos,
        column_height: i32,
        reach: i32,
    ) -> bool {
        let mut placed_any = false;
        let min = origin.offset(-reach, 0, -reach);
        let max = origin.offset(reach, 0, reach);

        for pos in BlockPos::between_closed(min, max) {
            let step_limit = Self::manhattan_distance(pos, origin);
            let column_pos =
                if Self::test_block_predicate(region, registry, &config.can_replace, pos) {
                    Self::find_stepped_column_surface(region, registry, config, pos, step_limit)
                } else {
                    Self::find_stepped_column_air(region, config, pos, step_limit)
                };
            let Some(mut cursor) = column_pos else {
                continue;
            };

            let mut blocks_left = column_height - step_limit / 2;
            while blocks_left >= 0 {
                if Self::test_block_predicate(region, registry, &config.can_replace, cursor) {
                    let state = Self::sample_block_state_provider(
                        region,
                        registry,
                        random,
                        &config.block,
                        cursor,
                    );
                    let _ = region.set_block_state(cursor, state, UpdateFlags::UPDATE_ALL);
                    cursor = cursor.above();
                    placed_any = true;
                } else {
                    if !Self::test_block_predicate(
                        region,
                        registry,
                        &config.continue_through,
                        cursor,
                    ) {
                        break;
                    }
                    cursor = cursor.above();
                }

                blocks_left -= 1;
            }
        }

        placed_any
    }

    fn find_stepped_column_surface(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        config: &SteppedColumnClusterConfiguration,
        mut cursor: BlockPos,
        mut limit: i32,
    ) -> Option<BlockPos> {
        while cursor.y() > region.min_y() + 1 && limit > 0 {
            limit -= 1;
            if Self::can_place_stepped_column_at(region, registry, config, cursor) {
                return Some(cursor);
            }
            cursor = cursor.below();
        }

        None
    }

    fn can_place_stepped_column_at(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        config: &SteppedColumnClusterConfiguration,
        pos: BlockPos,
    ) -> bool {
        if !Self::test_block_predicate(region, registry, &config.can_replace, pos) {
            return false;
        }

        let below = region.block_state(pos.below());
        !below.is_air()
            && !Self::block_matches_holder_set(below.get_block(), &config.cannot_place_on)
    }

    fn find_stepped_column_air(
        region: &WorldGenRegion<'_>,
        config: &SteppedColumnClusterConfiguration,
        mut cursor: BlockPos,
        mut limit: i32,
    ) -> Option<BlockPos> {
        while cursor.y() < region.max_y_exclusive() && limit > 0 {
            limit -= 1;
            let state = region.block_state(cursor);
            if Self::block_matches_holder_set(state.get_block(), &config.cannot_place_on) {
                return None;
            }
            if state.is_air() {
                return Some(cursor);
            }
            cursor = cursor.above();
        }

        None
    }
}
