#![expect(
    clippy::fn_params_excessive_bools,
    reason = "mushroom cap side flags mirror vanilla block properties"
)]

use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

#[derive(Clone, Copy)]
enum HugeMushroomKind {
    Brown,
    Red,
}

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_huge_brown_mushroom_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &HugeMushroomConfiguration,
        origin: BlockPos,
    ) -> bool {
        Self::place_huge_mushroom_feature(
            region,
            registry,
            random,
            config,
            origin,
            HugeMushroomKind::Brown,
        )
    }

    pub(in crate::worldgen::feature) fn place_huge_red_mushroom_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &HugeMushroomConfiguration,
        origin: BlockPos,
    ) -> bool {
        Self::place_huge_mushroom_feature(
            region,
            registry,
            random,
            config,
            origin,
            HugeMushroomKind::Red,
        )
    }

    fn place_huge_mushroom_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &HugeMushroomConfiguration,
        origin: BlockPos,
        kind: HugeMushroomKind,
    ) -> bool {
        let tree_height = Self::huge_mushroom_tree_height(random);
        if !Self::is_valid_huge_mushroom_position(
            region,
            registry,
            config,
            origin,
            tree_height,
            kind,
        ) {
            return false;
        }

        match kind {
            HugeMushroomKind::Brown => {
                Self::make_brown_mushroom_cap(
                    region,
                    registry,
                    random,
                    config,
                    origin,
                    tree_height,
                );
            }
            HugeMushroomKind::Red => {
                Self::make_red_mushroom_cap(region, registry, random, config, origin, tree_height);
            }
        }
        Self::place_huge_mushroom_trunk(region, registry, random, config, origin, tree_height);
        true
    }

    fn huge_mushroom_tree_height(random: &mut WorldgenRandom) -> i32 {
        let mut tree_height = random.next_i32_bounded(3) + 4;
        if random.next_i32_bounded(12) == 0 {
            tree_height *= 2;
        }
        tree_height
    }

    fn is_valid_huge_mushroom_position(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        config: &HugeMushroomConfiguration,
        origin: BlockPos,
        tree_height: i32,
        kind: HugeMushroomKind,
    ) -> bool {
        if origin.y() < region.min_y() + 1
            || origin.y() + tree_height + 2 > region.max_y_exclusive()
        {
            return false;
        }

        if !Self::test_block_predicate(region, registry, &config.can_place_on, origin.below()) {
            return false;
        }

        for dy in 0..=tree_height {
            let radius =
                Self::huge_mushroom_tree_radius_for_height(kind, -1, -1, config.foliage_radius, dy);
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let state = region.block_state(origin.offset(dx, dy, dz));
                    if !state.is_air()
                        && !registry
                            .blocks
                            .is_in_tag(state.get_block(), &vanilla_block_tags::LEAVES_TAG)
                    {
                        return false;
                    }
                }
            }
        }

        true
    }

    fn make_brown_mushroom_cap(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &HugeMushroomConfiguration,
        origin: BlockPos,
        tree_height: i32,
    ) {
        let radius = config.foliage_radius;
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let min_x = dx == -radius;
                let max_x = dx == radius;
                let min_z = dz == -radius;
                let max_z = dz == radius;
                let x_edge = min_x || max_x;
                let z_edge = min_z || max_z;
                if x_edge && z_edge {
                    continue;
                }

                let pos = origin.offset(dx, tree_height, dz);
                let west = min_x || z_edge && dx == 1 - radius;
                let east = max_x || z_edge && dx == radius - 1;
                let north = min_z || x_edge && dz == 1 - radius;
                let south = max_z || x_edge && dz == radius - 1;
                let mut state = Self::sample_block_state_provider(
                    region,
                    registry,
                    random,
                    &config.cap_provider,
                    origin,
                );

                if Self::has_mushroom_horizontal_properties(state) {
                    state =
                        Self::set_mushroom_horizontal_properties(state, west, east, north, south);
                }

                Self::place_huge_mushroom_block(region, registry, pos, state);
            }
        }
    }

    fn make_red_mushroom_cap(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &HugeMushroomConfiguration,
        origin: BlockPos,
        tree_height: i32,
    ) {
        for dy in tree_height - 3..=tree_height {
            let radius = if dy < tree_height {
                config.foliage_radius
            } else {
                config.foliage_radius - 1
            };
            let center = config.foliage_radius - 2;

            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let min_x = dx == -radius;
                    let max_x = dx == radius;
                    let min_z = dz == -radius;
                    let max_z = dz == radius;
                    let x_edge = min_x || max_x;
                    let z_edge = min_z || max_z;
                    if dy < tree_height && x_edge == z_edge {
                        continue;
                    }

                    let pos = origin.offset(dx, dy, dz);
                    let mut state = Self::sample_block_state_provider(
                        region,
                        registry,
                        random,
                        &config.cap_provider,
                        origin,
                    );

                    if Self::has_mushroom_horizontal_properties(state)
                        && state.try_get_value(&BlockStateProperties::UP).is_some()
                    {
                        state = state.set_value(&BlockStateProperties::UP, dy >= tree_height - 1);
                        state = Self::set_mushroom_horizontal_properties(
                            state,
                            dx < -center,
                            dx > center,
                            dz < -center,
                            dz > center,
                        );
                    }

                    Self::place_huge_mushroom_block(region, registry, pos, state);
                }
            }
        }
    }

    fn place_huge_mushroom_trunk(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &HugeMushroomConfiguration,
        origin: BlockPos,
        tree_height: i32,
    ) {
        for dy in 0..tree_height {
            let pos = origin.above_n(dy);
            let state = Self::sample_block_state_provider(
                region,
                registry,
                random,
                &config.stem_provider,
                origin,
            );
            Self::place_huge_mushroom_block(region, registry, pos, state);
        }
    }

    fn place_huge_mushroom_block(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        pos: BlockPos,
        state: BlockStateId,
    ) {
        let current_state = region.block_state(pos);
        if current_state.is_air()
            || registry.blocks.is_in_tag(
                current_state.get_block(),
                &vanilla_block_tags::REPLACEABLE_BY_MUSHROOMS_TAG,
            )
        {
            let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_ALL);
        }
    }

    fn has_mushroom_horizontal_properties(state: BlockStateId) -> bool {
        state.try_get_value(&BlockStateProperties::WEST).is_some()
            && state.try_get_value(&BlockStateProperties::EAST).is_some()
            && state.try_get_value(&BlockStateProperties::NORTH).is_some()
            && state.try_get_value(&BlockStateProperties::SOUTH).is_some()
    }

    fn set_mushroom_horizontal_properties(
        mut state: BlockStateId,
        west: bool,
        east: bool,
        north: bool,
        south: bool,
    ) -> BlockStateId {
        state = state.set_value(&BlockStateProperties::WEST, west);
        state = state.set_value(&BlockStateProperties::EAST, east);
        state = state.set_value(&BlockStateProperties::NORTH, north);
        state.set_value(&BlockStateProperties::SOUTH, south)
    }

    const fn huge_mushroom_tree_radius_for_height(
        kind: HugeMushroomKind,
        _trunk_height: i32,
        tree_height: i32,
        leaf_radius: i32,
        y_offset: i32,
    ) -> i32 {
        match kind {
            HugeMushroomKind::Brown => {
                if y_offset <= 3 {
                    0
                } else {
                    leaf_radius
                }
            }
            HugeMushroomKind::Red => {
                if (y_offset < tree_height && y_offset >= tree_height - 3)
                    || y_offset == tree_height
                {
                    leaf_radius
                } else {
                    0
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn huge_red_mushroom_validation_radius_matches_vanilla_sentinel_call() {
        assert_eq!(
            FeatureDecorationRunner::huge_mushroom_tree_radius_for_height(
                HugeMushroomKind::Red,
                -1,
                -1,
                3,
                4,
            ),
            0
        );
    }

    #[test]
    fn huge_brown_mushroom_validation_radius_ignores_tree_height_like_vanilla() {
        assert_eq!(
            FeatureDecorationRunner::huge_mushroom_tree_radius_for_height(
                HugeMushroomKind::Brown,
                -1,
                -1,
                3,
                4,
            ),
            3
        );
    }
}
