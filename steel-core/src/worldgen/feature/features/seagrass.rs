use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_seagrass_feature(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        config: &SeagrassConfiguration,
        origin: BlockPos,
    ) -> bool {
        let x = random.next_i32_bounded(8) - random.next_i32_bounded(8);
        let z = random.next_i32_bounded(8) - random.next_i32_bounded(8);
        let y = region.height_at(HeightmapType::OceanFloor, origin.x() + x, origin.z() + z);
        let grass_pos = BlockPos::new(origin.x() + x, y, origin.z() + z);

        if region.block_state(grass_pos).get_block() != &vanilla_blocks::WATER {
            return false;
        }

        let is_tall = random.next_f64() < f64::from(config.probability);
        let state = if is_tall {
            vanilla_blocks::TALL_SEAGRASS.default_state().set_value(
                &BlockStateProperties::DOUBLE_BLOCK_HALF,
                DoubleBlockHalf::Lower,
            )
        } else {
            vanilla_blocks::SEAGRASS.default_state()
        };
        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        if !behavior.can_survive(state, region, grass_pos) {
            return false;
        }

        if is_tall {
            let upper_pos = grass_pos.above();
            if region.block_state(upper_pos).get_block() == &vanilla_blocks::WATER {
                let upper_state = state.set_value(
                    &BlockStateProperties::DOUBLE_BLOCK_HALF,
                    DoubleBlockHalf::Upper,
                );
                let _ = region.set_block_state(grass_pos, state, UpdateFlags::UPDATE_CLIENTS);
                let _ = region.set_block_state(upper_pos, upper_state, UpdateFlags::UPDATE_CLIENTS);
            }
        } else {
            let _ = region.set_block_state(grass_pos, state, UpdateFlags::UPDATE_CLIENTS);
        }

        true
    }
}
