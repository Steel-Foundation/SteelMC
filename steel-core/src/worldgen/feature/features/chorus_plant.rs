use crate::behavior::blocks::ChorusPlantBlock;

use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_chorus_plant_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        if !region.block_state(origin).is_air()
            || !registry.blocks.is_in_tag(
                region.block_state(origin.below()).get_block(),
                &vanilla_block_tags::SUPPORTS_CHORUS_PLANT_TAG,
            )
        {
            return false;
        }

        Self::generate_chorus_plant(region, random, origin, 8);
        true
    }

    fn generate_chorus_plant(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        target: BlockPos,
        max_horizontal_spread: i32,
    ) {
        let plant = vanilla_blocks::CHORUS_PLANT.default_state();
        let state = ChorusPlantBlock::state_with_connections(region, target, plant);
        let _ = region.set_block_state(target, state, UpdateFlags::UPDATE_CLIENTS);
        Self::grow_chorus_tree_recursive(region, random, target, target, max_horizontal_spread, 0);
    }

    fn grow_chorus_tree_recursive(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        current: BlockPos,
        start_pos: BlockPos,
        max_horizontal_spread: i32,
        depth: i32,
    ) {
        let plant = vanilla_blocks::CHORUS_PLANT.default_state();
        let mut height = random.next_i32_bounded(4) + 1;
        if depth == 0 {
            height += 1;
        }

        for i in 0..height {
            let target = current.above_n(i + 1);
            if !Self::chorus_all_neighbors_empty(region, target, None) {
                return;
            }

            let target_state = ChorusPlantBlock::state_with_connections(region, target, plant);
            let _ = region.set_block_state(target, target_state, UpdateFlags::UPDATE_CLIENTS);

            let below = target.below();
            let below_state = ChorusPlantBlock::state_with_connections(region, below, plant);
            let _ = region.set_block_state(below, below_state, UpdateFlags::UPDATE_CLIENTS);
        }

        let mut placed_stem = false;
        if depth < 4 {
            let mut stems = random.next_i32_bounded(4);
            if depth == 0 {
                stems += 1;
            }

            for _ in 0..stems {
                let direction = Self::random_horizontal_direction(random);
                let target = current.above_n(height).relative(direction);
                if (target.x() - start_pos.x()).abs() >= max_horizontal_spread
                    || (target.z() - start_pos.z()).abs() >= max_horizontal_spread
                    || !region.block_state(target).is_air()
                    || !region.block_state(target.below()).is_air()
                    || !Self::chorus_all_neighbors_empty(region, target, Some(direction.opposite()))
                {
                    continue;
                }

                placed_stem = true;
                let target_state = ChorusPlantBlock::state_with_connections(region, target, plant);
                let _ = region.set_block_state(target, target_state, UpdateFlags::UPDATE_CLIENTS);

                let back = target.relative(direction.opposite());
                let back_state = ChorusPlantBlock::state_with_connections(region, back, plant);
                let _ = region.set_block_state(back, back_state, UpdateFlags::UPDATE_CLIENTS);

                Self::grow_chorus_tree_recursive(
                    region,
                    random,
                    target,
                    start_pos,
                    max_horizontal_spread,
                    depth + 1,
                );
            }
        }

        if !placed_stem {
            let flower = vanilla_blocks::CHORUS_FLOWER
                .default_state()
                .set_value(&BlockStateProperties::AGE_5, 5);
            let _ = region.set_block_state(
                current.above_n(height),
                flower,
                UpdateFlags::UPDATE_CLIENTS,
            );
        }
    }

    fn chorus_all_neighbors_empty(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        ignore: Option<Direction>,
    ) -> bool {
        for direction in Self::VANILLA_HORIZONTAL_DIRECTIONS {
            if Some(direction) != ignore && !region.block_state(pos.relative(direction)).is_air() {
                return false;
            }
        }
        true
    }
}
