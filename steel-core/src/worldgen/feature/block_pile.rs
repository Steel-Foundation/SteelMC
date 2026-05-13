use super::prelude::*;
use super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(super) fn place_block_pile_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &BlockPileConfiguration,
        origin: BlockPos,
    ) -> bool {
        if origin.y() < region.min_y() + 5 {
            return false;
        }

        let x_radius = 2 + random.next_i32_bounded(2);
        let z_radius = 2 + random.next_i32_bounded(2);

        for x in origin.x() - x_radius..=origin.x() + x_radius {
            for y in origin.y()..=origin.y() + 1 {
                for z in origin.z() - z_radius..=origin.z() + z_radius {
                    let dx = origin.x() - x;
                    let dz = origin.z() - z;
                    let distance_squared = (dx * dx + dz * dz) as f32;
                    if distance_squared <= random.next_f32() * 10.0 - random.next_f32() * 6.0
                        || random.next_f32() < 0.031
                    {
                        Self::try_place_block_pile_block(
                            region,
                            registry,
                            random,
                            config,
                            BlockPos::new(x, y, z),
                        );
                    }
                }
            }
        }

        true
    }

    pub(super) fn try_place_block_pile_block(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &BlockPileConfiguration,
        pos: BlockPos,
    ) {
        if !region.block_state(pos).is_air() || !Self::block_pile_may_place_on(region, random, pos)
        {
            return;
        }

        let state = Self::sample_block_state_provider(
            region,
            registry,
            random,
            &config.state_provider,
            pos,
        );
        let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_NONE);
    }

    pub(super) fn block_pile_may_place_on(
        region: &WorldGenRegion<'_>,
        random: &mut Xoroshiro,
        pos: BlockPos,
    ) -> bool {
        let below = region.block_state(pos.below());
        if below.get_block() == &vanilla_blocks::DIRT_PATH {
            return random.next_bool();
        }

        below.is_face_sturdy(Direction::Up)
    }
}
