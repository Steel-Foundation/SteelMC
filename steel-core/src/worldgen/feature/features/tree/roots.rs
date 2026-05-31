#![expect(
    clippy::too_many_arguments,
    reason = "mangrove root simulation mirrors vanilla recursion state"
)]

use super::super::super::prelude::*;
use super::super::super::runner::FeatureDecorationRunner;
use super::TreePlacement;

impl FeatureDecorationRunner {
    pub(super) fn tree_root_origin(
        random: &mut WorldgenRandom,
        origin: BlockPos,
        root_placer: Option<&RootPlacer>,
    ) -> BlockPos {
        match root_placer {
            Some(RootPlacer::Mangrove(placer)) => {
                origin.above_n(placer.trunk_offset_y.sample(random))
            }
            None => origin,
        }
    }

    pub(super) fn place_tree_roots(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        origin: BlockPos,
        trunk_origin: BlockPos,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> bool {
        let Some(RootPlacer::Mangrove(placer)) = config.root_placer.as_ref() else {
            return true;
        };
        Self::place_mangrove_tree_roots(
            region,
            registry,
            random,
            origin,
            trunk_origin,
            placer,
            placement,
        )
    }

    fn place_mangrove_tree_roots(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        origin: BlockPos,
        trunk_origin: BlockPos,
        placer: &MangroveRootPlacer,
        placement: &mut TreePlacement,
    ) -> bool {
        let mut column_pos = origin;
        while column_pos.y() < trunk_origin.y() {
            if !Self::can_place_mangrove_tree_root(
                region,
                column_pos,
                &placer.mangrove_root_placement,
            ) {
                return false;
            }

            column_pos = column_pos.above();
        }

        let mut root_positions = vec![trunk_origin.below()];
        for direction in Self::VANILLA_HORIZONTAL_DIRECTIONS {
            let root_pos = trunk_origin.relative(direction);
            let mut positions_in_direction = Vec::new();
            if !Self::simulate_mangrove_tree_roots(
                region,
                random,
                root_pos,
                direction,
                trunk_origin,
                &mut positions_in_direction,
                0,
                &placer.mangrove_root_placement,
            ) {
                return false;
            }

            root_positions.extend(positions_in_direction);
            root_positions.push(root_pos);
        }

        for root_pos in root_positions {
            Self::place_mangrove_tree_root(region, registry, random, root_pos, placer, placement);
        }

        true
    }

    fn simulate_mangrove_tree_roots(
        region: &WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        root_pos: BlockPos,
        direction: Direction,
        root_origin: BlockPos,
        root_positions: &mut Vec<BlockPos>,
        layer: i32,
        placement: &MangroveRootPlacement,
    ) -> bool {
        let Ok(position_count) = i32::try_from(root_positions.len()) else {
            return false;
        };
        if layer == placement.max_root_length || position_count > placement.max_root_length {
            return false;
        }

        for pos in Self::potential_mangrove_root_positions(
            root_pos,
            direction,
            random,
            root_origin,
            placement,
        ) {
            if Self::can_place_mangrove_tree_root(region, pos, placement) {
                root_positions.push(pos);
                if !Self::simulate_mangrove_tree_roots(
                    region,
                    random,
                    pos,
                    direction,
                    root_origin,
                    root_positions,
                    layer + 1,
                    placement,
                ) {
                    return false;
                }
            }
        }

        true
    }

    fn potential_mangrove_root_positions(
        pos: BlockPos,
        previous_direction: Direction,
        random: &mut WorldgenRandom,
        root_origin: BlockPos,
        placement: &MangroveRootPlacement,
    ) -> Vec<BlockPos> {
        let below = pos.below();
        let next_to = pos.relative(previous_direction);
        let width = Self::manhattan_distance(pos, root_origin);

        if width > placement.max_root_width - 3 && width <= placement.max_root_width {
            if random.next_f32() < placement.random_skew_chance {
                vec![below, next_to.below()]
            } else {
                vec![below]
            }
        } else if width > placement.max_root_width
            || random.next_f32() < placement.random_skew_chance
        {
            vec![below]
        } else if random.next_bool() {
            vec![next_to]
        } else {
            vec![below]
        }
    }

    fn can_place_mangrove_tree_root(
        region: &WorldGenRegion<'_>,

        pos: BlockPos,
        placement: &MangroveRootPlacement,
    ) -> bool {
        Self::tree_valid_pos_or_tag(region, pos, &placement.can_grow_through)
    }

    fn place_mangrove_tree_root(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        pos: BlockPos,
        placer: &MangroveRootPlacer,
        placement: &mut TreePlacement,
    ) {
        if Self::block_matches_identifiers(
            registry,
            region.block_state(pos),
            &placer.mangrove_root_placement.muddy_roots_in,
        ) {
            let state = Self::sample_block_state_provider(
                region,
                registry,
                random,
                &placer.mangrove_root_placement.muddy_roots_provider,
                pos,
            );
            let state = Self::copy_waterlogged_from(region, pos, state);
            placement.set_root(region, pos, state);
            return;
        }

        if !Self::can_place_mangrove_tree_root(region, pos, &placer.mangrove_root_placement) {
            return;
        }

        let state =
            Self::sample_block_state_provider(region, registry, random, &placer.root_provider, pos);
        let state = Self::copy_waterlogged_from(region, pos, state);
        placement.set_root(region, pos, state);

        let above = pos.above();
        if random.next_f32() < placer.above_root_placement.above_root_placement_chance
            && region.block_state(above).is_air()
        {
            let state = Self::sample_block_state_provider(
                region,
                registry,
                random,
                &placer.above_root_placement.above_root_provider,
                above,
            );
            let state = Self::copy_waterlogged_from(region, above, state);
            placement.set_root(region, above, state);
        }
    }

    fn block_matches_identifiers(
        registry: &Registry,
        state: BlockStateId,
        blocks: &[Identifier],
    ) -> bool {
        blocks.iter().any(|block_key| {
            let Some(block) = registry.blocks.by_key(block_key) else {
                panic!("mangrove root placement references unknown block {block_key}");
            };
            state.get_block() == block
        })
    }
}
