use steel_registry::vanilla_block_tags::BlockTag;

use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_bamboo_feature(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        config: &BambooConfiguration,
        origin: BlockPos,
    ) -> bool {
        let mut placed = 0;
        let mut bamboo_pos = origin;

        if region.block_state(bamboo_pos).is_air() {
            let bamboo = vanilla_blocks::BAMBOO.default_state();
            let behavior = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::BAMBOO);
            if behavior.can_survive(bamboo, region, bamboo_pos) {
                let height = random.next_i32_bounded(12) + 5;
                if random.next_f32() < config.probability {
                    Self::place_bamboo_podzol_disc(region, random, origin);
                }

                let trunk = Self::bamboo_trunk_state();
                for _ in 0..height {
                    if !region.block_state(bamboo_pos).is_air() {
                        break;
                    }
                    let _ = region.set_block_state(bamboo_pos, trunk, UpdateFlags::UPDATE_CLIENTS);
                    bamboo_pos = bamboo_pos.above();
                }

                if bamboo_pos.y() - origin.y() >= 3 {
                    let _ = region.set_block_state(
                        bamboo_pos,
                        Self::bamboo_final_large_state(),
                        UpdateFlags::UPDATE_CLIENTS,
                    );
                    bamboo_pos = bamboo_pos.below();
                    let _ = region.set_block_state(
                        bamboo_pos,
                        Self::bamboo_top_large_state(),
                        UpdateFlags::UPDATE_CLIENTS,
                    );
                    bamboo_pos = bamboo_pos.below();
                    let _ = region.set_block_state(
                        bamboo_pos,
                        Self::bamboo_top_small_state(),
                        UpdateFlags::UPDATE_CLIENTS,
                    );
                }
            }

            placed += 1;
        }

        placed > 0
    }

    fn place_bamboo_podzol_disc(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) {
        let radius = random.next_i32_bounded(4) + 1;
        for x in origin.x() - radius..=origin.x() + radius {
            for z in origin.z() - radius..=origin.z() + radius {
                let dx = x - origin.x();
                let dz = z - origin.z();
                if dx * dx + dz * dz > radius * radius {
                    continue;
                }

                let y = region.height_at(HeightmapType::WorldSurface, x, z) - 1;
                let pos = BlockPos::new(x, y, z);
                let state = region.block_state(pos);
                if state
                    .get_block()
                    .has_tag(&BlockTag::BENEATH_BAMBOO_PODZOL_REPLACEABLE)
                {
                    let _ = region.set_block_state(
                        pos,
                        vanilla_blocks::PODZOL.default_state(),
                        UpdateFlags::UPDATE_CLIENTS,
                    );
                }
            }
        }
    }

    fn bamboo_trunk_state() -> BlockStateId {
        vanilla_blocks::BAMBOO
            .default_state()
            .set_value(&BlockStateProperties::AGE_1, 1)
            .set_value(&BlockStateProperties::BAMBOO_LEAVES, BambooLeaves::None)
            .set_value(&BlockStateProperties::STAGE, 0)
    }

    fn bamboo_final_large_state() -> BlockStateId {
        Self::bamboo_trunk_state()
            .set_value(&BlockStateProperties::BAMBOO_LEAVES, BambooLeaves::Large)
            .set_value(&BlockStateProperties::STAGE, 1)
    }

    fn bamboo_top_large_state() -> BlockStateId {
        Self::bamboo_trunk_state()
            .set_value(&BlockStateProperties::BAMBOO_LEAVES, BambooLeaves::Large)
    }

    fn bamboo_top_small_state() -> BlockStateId {
        Self::bamboo_trunk_state()
            .set_value(&BlockStateProperties::BAMBOO_LEAVES, BambooLeaves::Small)
    }
}
