use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_kelp_feature(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        let mut placed = 0;
        let y = region.height_at(HeightmapType::OceanFloor, origin.x(), origin.z());
        let mut kelp_pos = BlockPos::new(origin.x(), y, origin.z());

        if region.block_state(kelp_pos).get_block() != &vanilla_blocks::WATER {
            return false;
        }

        let kelp_head = vanilla_blocks::KELP.default_state();
        let kelp_plant = vanilla_blocks::KELP_PLANT.default_state();
        let kelp_plant_behavior = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::KELP_PLANT);
        let kelp_head_behavior = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::KELP);
        let height = 1 + random.next_i32_bounded(10);

        for h in 0..=height {
            if region.block_state(kelp_pos).get_block() == &vanilla_blocks::WATER
                && region.block_state(kelp_pos.above()).get_block() == &vanilla_blocks::WATER
                && kelp_plant_behavior.can_survive(kelp_plant, region, kelp_pos)
            {
                if h == height {
                    let state = Self::aged_kelp_head(kelp_head, random);
                    let _ = region.set_block_state(kelp_pos, state, UpdateFlags::UPDATE_CLIENTS);
                    placed += 1;
                } else {
                    let _ =
                        region.set_block_state(kelp_pos, kelp_plant, UpdateFlags::UPDATE_CLIENTS);
                }
            } else if h > 0 {
                let below = kelp_pos.below();
                if kelp_head_behavior.can_survive(kelp_head, region, below)
                    && region.block_state(below.below()).get_block() != &vanilla_blocks::KELP
                {
                    let state = Self::aged_kelp_head(kelp_head, random);
                    let _ = region.set_block_state(below, state, UpdateFlags::UPDATE_CLIENTS);
                    placed += 1;
                }
                break;
            }

            kelp_pos = kelp_pos.above();
        }

        placed > 0
    }

    fn aged_kelp_head(state: BlockStateId, random: &mut WorldgenRandom) -> BlockStateId {
        let age = (random.next_i32_bounded(4) + 20) as u8;
        state.set_value(&BlockStateProperties::AGE_25, age)
    }
}
