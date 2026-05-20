use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_freeze_top_layer_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) -> bool {
        let ice = vanilla_blocks::ICE.default_state();
        let snow = vanilla_blocks::SNOW.default_state();

        for dx in 0..16 {
            for dz in 0..16 {
                let x = origin.x() + dx;
                let z = origin.z() + dz;
                let y = region.height_at(HeightmapType::MotionBlocking, x, z);
                let top_pos = BlockPos::new(x, y, z);
                let below_pos = top_pos.below();
                let biome = Self::biome_at_block(region, registry, biome_zoom_seed, top_pos);

                if Self::should_freeze_in_biome(region, biome, below_pos, false) {
                    let _ = region.set_block_state(below_pos, ice, UpdateFlags::UPDATE_CLIENTS);
                }

                if Self::should_snow_in_biome(region, biome, top_pos) {
                    let _ = region.set_block_state(top_pos, snow, UpdateFlags::UPDATE_CLIENTS);
                    Self::set_snowy_below(region, below_pos);
                }
            }
        }

        true
    }

    fn set_snowy_below(region: &mut WorldGenRegion<'_>, below_pos: BlockPos) {
        let below_state = region.block_state(below_pos);
        if below_state
            .try_get_value(&BlockStateProperties::SNOWY)
            .is_none()
        {
            return;
        }

        let snowy_state = below_state.set_value(&BlockStateProperties::SNOWY, true);
        let _ = region.set_block_state(below_pos, snowy_state, UpdateFlags::UPDATE_CLIENTS);
    }
}
