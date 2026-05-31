use steel_registry::vanilla_block_tags::BlockTag;

use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_lake_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &LakeConfiguration,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) -> bool {
        if origin.y() <= region.min_y() + 4 {
            return false;
        }

        let origin = origin.offset(-8, -4, -8);
        let mut grid = vec![false; 16 * 16 * 8];
        let spots = random.next_i32_bounded(4) + 4;
        for _ in 0..spots {
            Self::carve_lake_ellipsoid(random, &mut grid);
        }

        let fluid =
            Self::sample_block_state_provider(region, registry, random, &config.fluid, origin);
        if !Self::lake_boundary_valid(region, &grid, origin, fluid) {
            return false;
        }

        Self::place_lake_contents(region, &grid, origin, fluid);
        Self::place_lake_barrier(region, registry, random, config, &grid, origin);
        if get_fluid_state_from_block(fluid).is_water() {
            Self::freeze_lake_surface(region, registry, biome_zoom_seed, origin);
        }

        true
    }

    fn carve_lake_ellipsoid(random: &mut WorldgenRandom, grid: &mut [bool]) {
        let x_radius = random.next_f64() * 6.0 + 3.0;
        let y_radius = random.next_f64() * 4.0 + 2.0;
        let z_radius = random.next_f64() * 6.0 + 3.0;
        let x_center = random.next_f64() * (16.0 - x_radius - 2.0) + 1.0 + x_radius / 2.0;
        let y_center = random.next_f64() * (8.0 - y_radius - 4.0) + 2.0 + y_radius / 2.0;
        let z_center = random.next_f64() * (16.0 - z_radius - 2.0) + 1.0 + z_radius / 2.0;

        for x in 1..15 {
            for z in 1..15 {
                for y in 1..7 {
                    let x_delta = (f64::from(x) - x_center) / (x_radius / 2.0);
                    let y_delta = (f64::from(y) - y_center) / (y_radius / 2.0);
                    let z_delta = (f64::from(z) - z_center) / (z_radius / 2.0);
                    if x_delta * x_delta + y_delta * y_delta + z_delta * z_delta < 1.0 {
                        grid[Self::lake_index(x, y, z)] = true;
                    }
                }
            }
        }
    }

    fn lake_boundary_valid(
        region: &WorldGenRegion<'_>,
        grid: &[bool],
        origin: BlockPos,
        fluid: BlockStateId,
    ) -> bool {
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..8 {
                    if !Self::lake_is_boundary(grid, x, y, z) {
                        continue;
                    }

                    let state = region.block_state(origin.offset(x, y, z));
                    if y >= 4 && !get_fluid_state_from_block(state).is_empty() {
                        return false;
                    }

                    if y < 4 && !state.is_solid() && state != fluid {
                        return false;
                    }
                }
            }
        }

        true
    }

    fn place_lake_contents(
        region: &mut WorldGenRegion<'_>,
        grid: &[bool],
        origin: BlockPos,
        fluid: BlockStateId,
    ) {
        let cave_air = vanilla_blocks::CAVE_AIR.default_state();
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..8 {
                    if !grid[Self::lake_index(x, y, z)] {
                        continue;
                    }

                    let pos = origin.offset(x, y, z);
                    if !Self::lake_can_replace_block(region.block_state(pos)) {
                        continue;
                    }

                    if y >= 4 {
                        let _ = region.set_block_state(pos, cave_air, UpdateFlags::UPDATE_CLIENTS);
                        let _ = region.schedule_block_tick_default(pos, cave_air.get_block(), 0);
                        Self::mark_above_for_postprocessing(region, pos);
                    } else {
                        let _ = region.set_block_state(pos, fluid, UpdateFlags::UPDATE_CLIENTS);
                    }
                }
            }
        }
    }

    fn place_lake_barrier(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &LakeConfiguration,
        grid: &[bool],
        origin: BlockPos,
    ) {
        let barrier =
            Self::sample_block_state_provider(region, registry, random, &config.barrier, origin);
        if barrier.is_air() {
            return;
        }

        for x in 0..16 {
            for z in 0..16 {
                for y in 0..8 {
                    if !Self::lake_is_boundary(grid, x, y, z)
                        || (y >= 4 && random.next_i32_bounded(2) == 0)
                    {
                        continue;
                    }

                    let pos = origin.offset(x, y, z);
                    let state = region.block_state(pos);
                    if state.is_solid()
                        && !state
                            .get_block()
                            .has_tag(&BlockTag::LAVA_POOL_STONE_CANNOT_REPLACE)
                    {
                        let _ = region.set_block_state(pos, barrier, UpdateFlags::UPDATE_CLIENTS);
                        Self::mark_above_for_postprocessing(region, pos);
                    }
                }
            }
        }
    }

    fn freeze_lake_surface(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        biome_zoom_seed: i64,
        origin: BlockPos,
    ) {
        let ice = vanilla_blocks::ICE.default_state();
        for x in 0..16 {
            for z in 0..16 {
                let pos = origin.offset(x, 4, z);
                if Self::should_freeze(region, registry, biome_zoom_seed, pos, false)
                    && Self::lake_can_replace_block(region.block_state(pos))
                {
                    let _ = region.set_block_state(pos, ice, UpdateFlags::UPDATE_CLIENTS);
                }
            }
        }
    }

    fn lake_is_boundary(grid: &[bool], x: i32, y: i32, z: i32) -> bool {
        !grid[Self::lake_index(x, y, z)]
            && ((x < 15 && grid[Self::lake_index(x + 1, y, z)])
                || (x > 0 && grid[Self::lake_index(x - 1, y, z)])
                || (z < 15 && grid[Self::lake_index(x, y, z + 1)])
                || (z > 0 && grid[Self::lake_index(x, y, z - 1)])
                || (y < 7 && grid[Self::lake_index(x, y + 1, z)])
                || (y > 0 && grid[Self::lake_index(x, y - 1, z)]))
    }

    fn lake_can_replace_block(state: BlockStateId) -> bool {
        !state
            .get_block()
            .has_tag(&BlockTag::FEATURES_CANNOT_REPLACE)
    }

    const fn lake_index(x: i32, y: i32, z: i32) -> usize {
        ((x * 16 + z) * 8 + y) as usize
    }
}
