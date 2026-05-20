use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_basalt_pillar_feature(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        if !region.block_state(origin).is_air() || region.block_state(origin.above()).is_air() {
            return false;
        }

        let basalt = vanilla_blocks::BASALT.default_state();
        let mut pos = origin;
        let mut place_north_hangoff = true;
        let mut place_south_hangoff = true;
        let mut place_west_hangoff = true;
        let mut place_east_hangoff = true;

        while region.block_state(pos).is_air() {
            if region.is_outside_build_height(pos.y()) {
                return true;
            }

            let _ = region.set_block_state(pos, basalt, UpdateFlags::UPDATE_CLIENTS);
            if place_north_hangoff {
                place_north_hangoff =
                    Self::place_basalt_pillar_hangoff(region, random, basalt, pos.north());
            }
            if place_south_hangoff {
                place_south_hangoff =
                    Self::place_basalt_pillar_hangoff(region, random, basalt, pos.south());
            }
            if place_west_hangoff {
                place_west_hangoff =
                    Self::place_basalt_pillar_hangoff(region, random, basalt, pos.west());
            }
            if place_east_hangoff {
                place_east_hangoff =
                    Self::place_basalt_pillar_hangoff(region, random, basalt, pos.east());
            }

            pos = pos.below();
        }

        pos = pos.above();
        Self::place_basalt_pillar_base_hangoff(region, random, basalt, pos.north());
        Self::place_basalt_pillar_base_hangoff(region, random, basalt, pos.south());
        Self::place_basalt_pillar_base_hangoff(region, random, basalt, pos.west());
        Self::place_basalt_pillar_base_hangoff(region, random, basalt, pos.east());
        pos = pos.below();

        for dx in -3i32..4 {
            for dz in -3i32..4 {
                let probability = dx.abs() * dz.abs();
                if random.next_i32_bounded(10) < 10 - probability {
                    let mut base_pos = pos.offset(dx, 0, dz);
                    let mut max_drop = 3;

                    while region.block_state(base_pos.below()).is_air() {
                        base_pos = base_pos.below();
                        max_drop -= 1;
                        if max_drop <= 0 {
                            break;
                        }
                    }

                    if !region.block_state(base_pos.below()).is_air() {
                        let _ =
                            region.set_block_state(base_pos, basalt, UpdateFlags::UPDATE_CLIENTS);
                    }
                }
            }
        }

        true
    }

    pub(in crate::worldgen::feature) fn place_basalt_pillar_base_hangoff(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        basalt: BlockStateId,
        pos: BlockPos,
    ) {
        if random.next_bool() {
            let _ = region.set_block_state(pos, basalt, UpdateFlags::UPDATE_CLIENTS);
        }
    }

    pub(in crate::worldgen::feature) fn place_basalt_pillar_hangoff(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        basalt: BlockStateId,
        pos: BlockPos,
    ) -> bool {
        if random.next_i32_bounded(10) == 0 {
            return false;
        }

        let _ = region.set_block_state(pos, basalt, UpdateFlags::UPDATE_CLIENTS);
        true
    }
}
