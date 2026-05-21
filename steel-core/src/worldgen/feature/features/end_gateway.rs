use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_end_gateway_feature(
        region: &WorldGenRegion<'_>,
        config: &EndGatewayConfiguration,
        origin: BlockPos,
    ) -> bool {
        for dy in -2..=2 {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    let pos = origin.offset(dx, dy, dz);
                    let same_x = dx == 0;
                    let same_y = dy == 0;
                    let same_z = dz == 0;
                    let end = dy.abs() == 2;
                    if same_x && same_y && same_z {
                        let state = vanilla_blocks::END_GATEWAY.default_state();
                        let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_ALL);
                        let exit = config.exit.map(|[x, y, z]| BlockPos::new(x, y, z));
                        Self::set_end_gateway_block_entity(region, pos, state, exit, config.exact);
                    } else if same_y {
                        let _ = region.set_block_state(
                            pos,
                            vanilla_blocks::AIR.default_state(),
                            UpdateFlags::UPDATE_ALL,
                        );
                    } else if (end && same_x && same_z) || ((same_x || same_z) && !end) {
                        let _ = region.set_block_state(
                            pos,
                            vanilla_blocks::BEDROCK.default_state(),
                            UpdateFlags::UPDATE_ALL,
                        );
                    } else {
                        let _ = region.set_block_state(
                            pos,
                            vanilla_blocks::AIR.default_state(),
                            UpdateFlags::UPDATE_ALL,
                        );
                    }
                }
            }
        }

        true
    }
}
