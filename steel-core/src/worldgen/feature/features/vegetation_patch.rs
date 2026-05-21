use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;
use super::super::vanilla_collections::JavaBlockPosSet;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_vegetation_patch_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &VegetationPatchConfiguration,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) -> bool {
        let x_radius = config.xz_radius.sample(random) + 1;
        let z_radius = config.xz_radius.sample(random) + 1;
        let surface = Self::place_vegetation_patch_ground(
            region, registry, random, config, origin, x_radius, z_radius,
        );
        Self::distribute_vegetation_patch(
            region,
            registry,
            random,
            config,
            &surface,
            biome_zoom_seed,
            false,
        );
        !surface.is_empty()
    }

    pub(in crate::worldgen::feature) fn place_waterlogged_vegetation_patch_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &VegetationPatchConfiguration,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) -> bool {
        let x_radius = config.xz_radius.sample(random) + 1;
        let z_radius = config.xz_radius.sample(random) + 1;
        let surface = Self::place_vegetation_patch_ground(
            region, registry, random, config, origin, x_radius, z_radius,
        );
        let water_surface = Self::waterlogged_vegetation_patch_surface(region, &surface);
        for surface_pos in water_surface.java_ordered_positions() {
            let _ = region.set_block_state(
                surface_pos,
                vanilla_blocks::WATER.default_state(),
                UpdateFlags::UPDATE_CLIENTS,
            );
        }

        Self::distribute_vegetation_patch(
            region,
            registry,
            random,
            config,
            &water_surface,
            biome_zoom_seed,
            true,
        );
        !water_surface.is_empty()
    }

    fn place_vegetation_patch_ground(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &VegetationPatchConfiguration,
        origin: BlockPos,
        x_radius: i32,
        z_radius: i32,
    ) -> JavaBlockPosSet {
        let inwards = Self::vegetation_patch_surface_direction(config.surface);
        let outwards = inwards.opposite();
        let mut surface = JavaBlockPosSet::default();

        for dx in -x_radius..=x_radius {
            let at_longitude_limit = dx == -x_radius || dx == x_radius;
            for dz in -z_radius..=z_radius {
                let at_latitude_limit = dz == -z_radius || dz == z_radius;
                let is_corner = at_longitude_limit && at_latitude_limit;
                let is_edge_but_not_corner =
                    (at_longitude_limit || at_latitude_limit) && !is_corner;

                if is_corner {
                    continue;
                }
                if is_edge_but_not_corner
                    && (config.extra_edge_column_chance == 0.0
                        || random.next_f32() > config.extra_edge_column_chance)
                {
                    continue;
                }

                let mut pos = origin.offset(dx, 0, dz);
                for _ in 0..config.vertical_range {
                    if !region.block_state(pos).is_air() {
                        break;
                    }
                    pos = pos.relative(inwards);
                }
                for _ in 0..config.vertical_range {
                    if region.block_state(pos).is_air() {
                        break;
                    }
                    pos = pos.relative(outwards);
                }

                let below_pos = pos.relative(inwards);
                let below_state = region.block_state(below_pos);
                if !region.block_state(pos).is_air() || !below_state.is_face_sturdy(outwards) {
                    continue;
                }

                let sampled_depth = config.depth.sample(random);
                let extra_depth = i32::from(
                    config.extra_bottom_block_chance > 0.0
                        && random.next_f32() < config.extra_bottom_block_chance,
                );
                let depth = sampled_depth + extra_depth;
                let mut ground_cursor = below_pos;
                if Self::place_vegetation_patch_ground_column(
                    region,
                    registry,
                    random,
                    config,
                    &mut ground_cursor,
                    inwards,
                    depth,
                ) {
                    surface.insert(below_pos);
                }
            }
        }

        surface
    }

    fn place_vegetation_patch_ground_column(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &VegetationPatchConfiguration,
        pos: &mut BlockPos,
        inwards: Direction,
        depth: i32,
    ) -> bool {
        for i in 0..depth {
            let state_to_place = Self::sample_block_state_provider(
                region,
                registry,
                random,
                &config.ground_state,
                *pos,
            );
            let current_state = region.block_state(*pos);
            if state_to_place.get_block() != current_state.get_block() {
                if !registry
                    .blocks
                    .is_in_tag(current_state.get_block(), &config.replaceable)
                {
                    return i != 0;
                }

                let _ = region.set_block_state(*pos, state_to_place, UpdateFlags::UPDATE_CLIENTS);
                *pos = (*pos).relative(inwards);
            }
        }

        true
    }

    fn distribute_vegetation_patch(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &VegetationPatchConfiguration,
        surface: &JavaBlockPosSet,
        biome_zoom_seed: i64,
        waterlogged: bool,
    ) {
        if config.vegetation_chance <= 0.0 {
            return;
        }

        let outwards = Self::vegetation_patch_surface_direction(config.surface).opposite();
        for surface_pos in surface.java_ordered_positions() {
            if random.next_f32() >= config.vegetation_chance {
                continue;
            }

            let vegetation_origin = if waterlogged {
                surface_pos.below().relative(outwards)
            } else {
                surface_pos.relative(outwards)
            };
            let placed = Self::place_placed_feature_ref(
                region,
                registry,
                random,
                vegetation_origin,
                &config.vegetation_feature,
                biome_zoom_seed,
            );
            if waterlogged && placed {
                let state = region.block_state(surface_pos);
                if let Some(false) = state.try_get_value(&BlockStateProperties::WATERLOGGED) {
                    let _ = region.set_block_state(
                        surface_pos,
                        state.set_value(&BlockStateProperties::WATERLOGGED, true),
                        UpdateFlags::UPDATE_CLIENTS,
                    );
                }
            }
        }
    }

    fn waterlogged_vegetation_patch_surface(
        region: &WorldGenRegion<'_>,
        surface: &JavaBlockPosSet,
    ) -> JavaBlockPosSet {
        let mut water_surface = JavaBlockPosSet::default();
        for pos in surface.java_ordered_positions() {
            if !Self::vegetation_patch_surface_exposed(region, pos) {
                water_surface.insert(pos);
            }
        }
        water_surface
    }

    fn vegetation_patch_surface_exposed(region: &WorldGenRegion<'_>, pos: BlockPos) -> bool {
        [
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
            Direction::Down,
        ]
        .into_iter()
        .any(|direction| {
            !region
                .block_state(pos.relative(direction))
                .is_face_sturdy(direction.opposite())
        })
    }

    const fn vegetation_patch_surface_direction(surface: VerticalSurface) -> Direction {
        match surface {
            VerticalSurface::Floor => Direction::Down,
            VerticalSurface::Ceiling => Direction::Up,
        }
    }
}
