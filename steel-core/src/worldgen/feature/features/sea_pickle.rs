use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_sea_pickle_feature(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        config: &SeaPickleConfiguration,
        origin: BlockPos,
    ) -> bool {
        let mut placed = 0;
        let count = config.count.sample(random);

        for _ in 0..count {
            let x = random.next_i32_bounded(8) - random.next_i32_bounded(8);
            let z = random.next_i32_bounded(8) - random.next_i32_bounded(8);
            let y = region.height_at(HeightmapType::OceanFloor, origin.x() + x, origin.z() + z);
            let pickle_pos = BlockPos::new(origin.x() + x, y, origin.z() + z);
            let pickle_count = (random.next_i32_bounded(4) + 1) as u8;
            let pickle_state = vanilla_blocks::SEA_PICKLE
                .default_state()
                .set_value(&BlockStateProperties::PICKLES, pickle_count);
            let behavior = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::SEA_PICKLE);

            if region.block_state(pickle_pos).get_block() == &vanilla_blocks::WATER
                && behavior.can_survive(pickle_state, region, pickle_pos)
            {
                let _ =
                    region.set_block_state(pickle_pos, pickle_state, UpdateFlags::UPDATE_CLIENTS);
                placed += 1;
            }
        }

        placed > 0
    }
}
