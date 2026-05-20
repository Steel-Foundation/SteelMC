#![expect(
    clippy::too_many_lines,
    reason = "desert well placement is kept linear to mirror vanilla"
)]

use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

const DESERT_WELL_ARCHAEOLOGY: &str = "minecraft:archaeology/desert_well";

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_desert_well_feature(
        region: &WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        let mut origin = origin.above();
        while region.block_state(origin).is_air() && origin.y() > region.min_y() + 2 {
            origin = origin.below();
        }

        if region.block_state(origin).get_block() != &vanilla_blocks::SAND {
            return false;
        }

        for ox in -2..=2 {
            for oz in -2..=2 {
                if region.block_state(origin.offset(ox, -1, oz)).is_air()
                    && region.block_state(origin.offset(ox, -2, oz)).is_air()
                {
                    return false;
                }
            }
        }

        let sandstone = vanilla_blocks::SANDSTONE.default_state();
        let sand = vanilla_blocks::SAND.default_state();
        let sand_slab = vanilla_blocks::SANDSTONE_SLAB.default_state();
        let water = vanilla_blocks::WATER.default_state();

        for oy in -2..=0 {
            for ox in -2..=2 {
                for oz in -2..=2 {
                    let _ = region.set_block_state(
                        origin.offset(ox, oy, oz),
                        sandstone,
                        UpdateFlags::UPDATE_CLIENTS,
                    );
                }
            }
        }

        let _ = region.set_block_state(origin, water, UpdateFlags::UPDATE_CLIENTS);
        for direction in Self::VANILLA_HORIZONTAL_DIRECTIONS {
            let _ = region.set_block_state(
                origin.relative(direction),
                water,
                UpdateFlags::UPDATE_CLIENTS,
            );
        }

        let sand_center = origin.below();
        let _ = region.set_block_state(sand_center, sand, UpdateFlags::UPDATE_CLIENTS);
        for direction in Self::VANILLA_HORIZONTAL_DIRECTIONS {
            let _ = region.set_block_state(
                sand_center.relative(direction),
                sand,
                UpdateFlags::UPDATE_CLIENTS,
            );
        }

        for ox in -2..=2 {
            for oz in -2..=2 {
                if ox == -2 || ox == 2 || oz == -2 || oz == 2 {
                    let _ = region.set_block_state(
                        origin.offset(ox, 1, oz),
                        sandstone,
                        UpdateFlags::UPDATE_CLIENTS,
                    );
                }
            }
        }

        let _ = region.set_block_state(
            origin.offset(2, 1, 0),
            sand_slab,
            UpdateFlags::UPDATE_CLIENTS,
        );
        let _ = region.set_block_state(
            origin.offset(-2, 1, 0),
            sand_slab,
            UpdateFlags::UPDATE_CLIENTS,
        );
        let _ = region.set_block_state(
            origin.offset(0, 1, 2),
            sand_slab,
            UpdateFlags::UPDATE_CLIENTS,
        );
        let _ = region.set_block_state(
            origin.offset(0, 1, -2),
            sand_slab,
            UpdateFlags::UPDATE_CLIENTS,
        );

        for ox in -1..=1 {
            for oz in -1..=1 {
                let state = if ox == 0 && oz == 0 {
                    sandstone
                } else {
                    sand_slab
                };
                let _ = region.set_block_state(
                    origin.offset(ox, 4, oz),
                    state,
                    UpdateFlags::UPDATE_CLIENTS,
                );
            }
        }

        for oy in 1..=3 {
            let _ = region.set_block_state(
                origin.offset(-1, oy, -1),
                sandstone,
                UpdateFlags::UPDATE_CLIENTS,
            );
            let _ = region.set_block_state(
                origin.offset(-1, oy, 1),
                sandstone,
                UpdateFlags::UPDATE_CLIENTS,
            );
            let _ = region.set_block_state(
                origin.offset(1, oy, -1),
                sandstone,
                UpdateFlags::UPDATE_CLIENTS,
            );
            let _ = region.set_block_state(
                origin.offset(1, oy, 1),
                sandstone,
                UpdateFlags::UPDATE_CLIENTS,
            );
        }

        let water_positions = [
            origin,
            origin.east(),
            origin.south(),
            origin.west(),
            origin.north(),
        ];
        let first = water_positions[random.next_i32_bounded(5) as usize].below();
        Self::place_suspicious_sand(region, first);
        let second = water_positions[random.next_i32_bounded(5) as usize].below_n(2);
        Self::place_suspicious_sand(region, second);
        true
    }

    fn place_suspicious_sand(region: &WorldGenRegion<'_>, pos: BlockPos) {
        let state = vanilla_blocks::SUSPICIOUS_SAND.default_state();
        if region.set_block_state(pos, state, UpdateFlags::UPDATE_ALL) {
            Self::set_brushable_loot_table(region, pos, state, DESERT_WELL_ARCHAEOLOGY);
        }
    }
}
