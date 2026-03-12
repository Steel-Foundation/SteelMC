use std::marker::PhantomData;

use sha2::{Digest, Sha256};
use steel_registry::REGISTRY;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::noise_parameters::get_noise_parameters;
use steel_registry::vanilla_biomes;
use steel_utils::BlockStateId;
use steel_utils::density::{ColumnCache, DimensionNoises, NoiseSettings};
use steel_utils::math::noise_math::lerp2;
use steel_utils::random::{Random, RandomSplitter, xoroshiro::Xoroshiro};
use steel_utils::surface::SurfaceRuleContext;

use crate::chunk::aquifer::{Aquifer, AquiferResult, preliminary_surface_level};
use crate::chunk::beardifier::Beardifier;
use crate::chunk::chunk_access::ChunkAccess;
use crate::chunk::chunk_generator::ChunkGenerator;
use crate::chunk::heightmap::HeightmapType;
use crate::chunk::noise_chunk::NoiseChunk;
use crate::chunk::ore_veinifier::OreVeinifier;
use crate::chunk::section::Sections;
use crate::chunk::surface_system::SurfaceSystem;
use crate::worldgen::BiomeSourceKind;

/// A chunk generator for vanilla (normal) world generation.
///
/// Matches vanilla's `NoiseBasedChunkGenerator`. The biome source is pluggable
/// per-dimension — overworld, nether, and end each provide a different
/// [`BiomeSourceKind`] variant.
///
/// Generic over `N: DimensionNoises` to support different dimensions with
/// their own transpiled density functions and noise settings.
pub struct VanillaGenerator<N: DimensionNoises> {
    /// Biome source for this dimension. Determines biomes at each quart position.
    biome_source: BiomeSourceKind,
    /// Noise generators for this dimension's density functions.
    /// Boxed because noise structs can be large.
    noises: Box<N>,
    /// Seed positional splitter for per-chunk construction of aquifers.
    splitter: RandomSplitter,
    /// Ore vein generator for replacing stone with ore blocks.
    ore_veinifier: Option<OreVeinifier>,
    /// Surface system for biome-specific block replacement.
    surface_system: SurfaceSystem,
    /// Block state ID for the default block, cached at construction time.
    default_block_id: BlockStateId,
    /// Obfuscated seed for `BiomeManager` biome zoom fuzzing.
    biome_zoom_seed: i64,
    _phantom: PhantomData<N>,
}

impl<N: DimensionNoises> VanillaGenerator<N> {
    /// Creates a new `VanillaGenerator` with the given biome source and seed.
    ///
    /// # Panics
    /// Panics if SHA-256 hash output is shorter than 8 bytes (cannot happen).
    #[must_use]
    pub fn new(biome_source: BiomeSourceKind, seed: u64) -> Self {
        let mut rng = Xoroshiro::from_seed(seed);
        let splitter = rng.next_positional();
        let noise_params = get_noise_parameters();
        let noises = N::create(seed, &splitter, &noise_params);

        let ore_veinifier = if N::Settings::ORE_VEINS_ENABLED {
            Some(OreVeinifier::new(&splitter))
        } else {
            None
        };

        let default_block_id = N::Settings::default_block_id();
        let surface_system = SurfaceSystem::new(
            &splitter,
            &noise_params,
            N::surface_noise_ids(),
            default_block_id,
            N::Settings::SEA_LEVEL,
        );

        // BiomeManager.obfuscateSeed(seed) — Guava's Hashing.sha256().hashLong(seed).asLong()
        // Guava uses little-endian for both input (putLong) and output (asLong).
        let biome_zoom_seed = {
            let mut hasher = Sha256::new();
            hasher.update((seed as i64).to_le_bytes());
            let result = hasher.finalize();
            i64::from_le_bytes(result[0..8].try_into().expect("SHA-256 produces 32 bytes"))
        };

        Self {
            biome_source,
            noises: Box::new(noises),
            splitter,
            ore_veinifier,
            surface_system,
            default_block_id,
            biome_zoom_seed,
            _phantom: PhantomData,
        }
    }
}

impl<N: DimensionNoises> ChunkGenerator for VanillaGenerator<N> {
    fn create_structures(&self, _chunk: &ChunkAccess) {}

    fn create_biomes(&self, chunk: &ChunkAccess) {
        let pos = chunk.pos();
        let min_y = chunk.min_y();
        let section_count = chunk.sections().sections.len();

        let chunk_x = pos.0.x;
        let chunk_z = pos.0.y;

        let mut sampler = self.biome_source.chunk_sampler();

        // Match vanilla's iteration order: Section(Y) → X → Y → Z.
        // This is critical because the R-tree biome cache (persistent warm-start)
        // determines tie-breaking for equal-distance entries, and the cache state
        // depends on the order of biome lookups.
        for section_index in 0..section_count {
            let section_y = (min_y / 16) + section_index as i32;
            let section = &chunk.sections().sections[section_index];
            let mut section_guard = section.write();

            for local_quart_x in 0..4i32 {
                let quart_x = chunk_x * 4 + local_quart_x;

                for local_quart_y in 0..4i32 {
                    let quart_y = section_y * 4 + local_quart_y;

                    for local_quart_z in 0..4i32 {
                        let quart_z = chunk_z * 4 + local_quart_z;

                        let biome = sampler.sample(quart_x, quart_y, quart_z);
                        let biome_id = *REGISTRY.biomes.get_id(biome) as u16;

                        section_guard.biomes.set(
                            local_quart_x as usize,
                            local_quart_y as usize,
                            local_quart_z as usize,
                            biome_id,
                        );
                    }
                }
            }
        }

        chunk.mark_dirty();
    }

    fn fill_from_noise(&self, chunk: &ChunkAccess) {
        let pos = chunk.pos();
        let chunk_min_x = pos.0.x * 16;
        let chunk_min_z = pos.0.y * 16;

        let min_y = N::Settings::MIN_Y;
        let height = N::Settings::HEIGHT;

        let mut noise_chunk = NoiseChunk::<N>::new(chunk_min_x, chunk_min_z);
        let noises = &*self.noises;

        let mut column_cache = N::ColumnCache::default();
        column_cache.init_grid(chunk_min_x, chunk_min_z, noises);

        let default_block_id = self.default_block_id;
        let ore_veinifier = &self.ore_veinifier;
        let mut aquifer = Aquifer::<N>::new(
            chunk_min_x,
            chunk_min_z,
            min_y,
            height,
            &self.splitter,
            noises,
            // Aquifer samples at arbitrary (x,z) outside the chunk, so it needs its own cache
            column_cache.clone(),
        );

        let structure_starts = chunk.structure_starts();
        let beardifier = Beardifier::for_structures_in_chunk(&structure_starts, pos.0.x, pos.0.y);
        let beard_opt = if beardifier.is_empty() {
            None
        } else {
            Some(&beardifier)
        };

        noise_chunk.fill(
            noises,
            &mut column_cache,
            beard_opt,
            |local_x, world_y, local_z, density, interpolated, cache| {
                let relative_y = (world_y - min_y) as usize;
                let world_x = chunk_min_x + local_x as i32;
                let world_z = chunk_min_z + local_z as i32;

                match aquifer.compute_substance(noises, world_x, world_y, world_z, density) {
                    AquiferResult::Solid => {
                        let block = ore_veinifier
                            .as_ref()
                            .and_then(|ov| {
                                ov.compute_interpolated(
                                    noises,
                                    cache,
                                    interpolated,
                                    world_x,
                                    world_y,
                                    world_z,
                                )
                            })
                            .unwrap_or(default_block_id);
                        chunk.set_relative_block(local_x, relative_y, local_z, block);
                    }
                    AquiferResult::Fluid(id) => {
                        chunk.set_relative_block(local_x, relative_y, local_z, id);
                    }
                    AquiferResult::Air => {}
                }
            },
        );
    }

    #[allow(clippy::too_many_lines)]
    fn build_surface(&self, chunk: &ChunkAccess, neighbor_biomes: &dyn Fn(i32, i32, i32) -> u16) {
        let min_y = N::Settings::MIN_Y;
        let pos = chunk.pos();
        let chunk_min_x = pos.0.x * 16;
        let chunk_min_z = pos.0.y * 16;
        let default_block_id = self.default_block_id;
        let noises = &*self.noises;
        let chunk_quart_x = pos.0.x * 4;
        let chunk_quart_z = pos.0.y * 4;

        // Ensure worldgen heightmaps are primed (fill_from_noise uses set_relative_block
        // which doesn't update heightmaps).
        chunk.prime_worldgen_heightmaps();

        // Pre-compute the 4 preliminary surface level corners for the 16-block cell.
        // Vanilla uses bilinear interpolation across these 4 corners (SurfaceRules.Context).
        let mut psl_cache = N::ColumnCache::default();
        let p00 = preliminary_surface_level::<N>(noises, &mut psl_cache, chunk_min_x, chunk_min_z);
        let p10 =
            preliminary_surface_level::<N>(noises, &mut psl_cache, chunk_min_x + 16, chunk_min_z);
        let p01 =
            preliminary_surface_level::<N>(noises, &mut psl_cache, chunk_min_x, chunk_min_z + 16);
        let p11 = preliminary_surface_level::<N>(
            noises,
            &mut psl_cache,
            chunk_min_x + 16,
            chunk_min_z + 16,
        );

        // Read WorldSurfaceWg heightmap once
        let heightmaps = chunk.proto_heightmaps();
        let worldgen_surface = heightmaps
            .get(HeightmapType::WorldSurfaceWg)
            .expect("WorldSurfaceWg heightmap not initialized");

        let eroded_badlands_id = *REGISTRY.biomes.get_id(&vanilla_biomes::ERODED_BADLANDS) as u16;

        for local_x in 0..16usize {
            for local_z in 0..16usize {
                let block_x = chunk_min_x + local_x as i32;
                let block_z = chunk_min_z + local_z as i32;

                // Start scanning from one above the highest non-air block
                let mut start_height = worldgen_surface.get_first_available(local_x, local_z);

                // Eroded badlands extension: add terracotta pillars above surface
                let surface_biome_id = get_fuzzed_biome(
                    chunk.sections(),
                    self.biome_zoom_seed,
                    block_x,
                    start_height,
                    block_z,
                    min_y,
                    chunk_quart_x,
                    chunk_quart_z,
                    neighbor_biomes,
                );
                if surface_biome_id == eroded_badlands_id {
                    start_height = self.surface_system.eroded_badlands_extension(
                        chunk,
                        local_x,
                        local_z,
                        block_x,
                        block_z,
                        start_height,
                        min_y,
                    );
                }

                // Surface depth for this column
                let surface_depth = self.surface_system.get_surface_depth(block_x, block_z);

                // Surface secondary noise (lazy in vanilla, but always used in overworld)
                let surface_secondary = self.surface_system.get_surface_secondary(block_x, block_z);

                // Min surface level: bilinear interpolation of preliminary surface level
                // Vanilla: (float)(blockX & 15) / 16.0F — float intermediate is exact for 0-15
                let t_x = f64::from(local_x as u8) / 16.0;
                let t_z = f64::from(local_z as u8) / 16.0;
                let interp = lerp2(
                    t_x,
                    t_z,
                    f64::from(p00),
                    f64::from(p10),
                    f64::from(p01),
                    f64::from(p11),
                );
                let min_surface_level = interp.floor() as i32 + surface_depth - 8;

                // Steep condition: check if any neighboring column differs by >= 4
                let steep = {
                    let z_north = local_z.saturating_sub(1);
                    let z_south = (local_z + 1).min(15);
                    let h_north = worldgen_surface.get_highest_taken(local_x, z_north);
                    let h_south = worldgen_surface.get_highest_taken(local_x, z_south);
                    if h_south >= h_north + 4 {
                        true
                    } else {
                        let x_west = local_x.saturating_sub(1);
                        let x_east = (local_x + 1).min(15);
                        let h_west = worldgen_surface.get_highest_taken(x_west, local_z);
                        let h_east = worldgen_surface.get_highest_taken(x_east, local_z);
                        h_west >= h_east + 4
                    }
                };

                let mut stone_depth_above: i32 = 0;
                let mut water_height: i32 = i32::MIN;
                let mut next_ceiling_stone_y: i32 = i32::MAX;

                for y in (min_y..=start_height).rev() {
                    let relative_y = (y - min_y) as usize;
                    let state = chunk
                        .get_relative_block(local_x, relative_y, local_z)
                        .unwrap_or(BlockStateId(0));

                    if state.is_air() {
                        stone_depth_above = 0;
                        water_height = i32::MIN;
                        continue;
                    }

                    if state.get_block().config.liquid {
                        if water_height == i32::MIN {
                            water_height = y + 1;
                        }
                        continue;
                    }

                    // Solid block — scan for stone_depth_below (lookahead)
                    if next_ceiling_stone_y >= y {
                        next_ceiling_stone_y = i32::MIN;
                        for la_y in (min_y - 1..y).rev() {
                            if la_y < min_y {
                                next_ceiling_stone_y = la_y + 1;
                                break;
                            }
                            let la_rel = (la_y - min_y) as usize;
                            let la_state = chunk
                                .get_relative_block(local_x, la_rel, local_z)
                                .unwrap_or(BlockStateId(0));
                            // isStone = !isAir && !isLiquid
                            if la_state.is_air() || la_state.get_block().config.liquid {
                                next_ceiling_stone_y = la_y + 1;
                                break;
                            }
                        }
                    }

                    stone_depth_above += 1;
                    let stone_depth_below = y - next_ceiling_stone_y + 1;

                    // Only apply surface rules to the default block
                    if state == default_block_id {
                        // Get biome via fuzzed BiomeManager lookup
                        let biome_id = get_fuzzed_biome(
                            chunk.sections(),
                            self.biome_zoom_seed,
                            block_x,
                            y,
                            block_z,
                            min_y,
                            chunk_quart_x,
                            chunk_quart_z,
                            neighbor_biomes,
                        );

                        let cold_enough_to_snow = self
                            .surface_system
                            .cold_enough_to_snow(biome_id, block_x, y, block_z);

                        let ctx = SurfaceRuleContext {
                            block_x,
                            block_z,
                            surface_depth,
                            surface_secondary,
                            min_surface_level,
                            steep,
                            block_y: y,
                            stone_depth_above,
                            stone_depth_below,
                            water_height,
                            biome_id,
                            cold_enough_to_snow,
                            system: &self.surface_system,
                        };

                        let rule_result = N::try_apply_surface_rule(&ctx);

                        if let Some(new_block) = rule_result {
                            chunk.set_relative_block(local_x, relative_y, local_z, new_block);
                        }
                    }
                }
            }
        }
    }

    fn apply_carvers(&self, _chunk: &ChunkAccess) {}

    fn apply_biome_decorations(&self, _chunk: &ChunkAccess) {}
}

// ── BiomeManager biome zoom helpers ──────────────────────────────────────────

/// Vanilla's `LinearCongruentialGenerator.next()`.
#[inline]
const fn lcg_next(mut rval: i64, c: i64) -> i64 {
    rval = rval.wrapping_mul(
        rval.wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407),
    );
    rval = rval.wrapping_add(c);
    rval
}

/// Vanilla's `BiomeManager.getFiddle()`.
#[inline]
fn get_fiddle(rval: i64) -> f64 {
    let uniform = ((rval >> 24).rem_euclid(1024)) as f64 / 1024.0;
    (uniform - 0.5) * 0.9
}

/// Vanilla's `BiomeManager.getFiddledDistance()`.
fn get_fiddled_distance(seed: i64, x: i32, y: i32, z: i32, dx: f64, dy: f64, dz: f64) -> f64 {
    let mut rval = lcg_next(seed, i64::from(x));
    rval = lcg_next(rval, i64::from(y));
    rval = lcg_next(rval, i64::from(z));
    rval = lcg_next(rval, i64::from(x));
    rval = lcg_next(rval, i64::from(y));
    rval = lcg_next(rval, i64::from(z));
    let fx = get_fiddle(rval);
    rval = lcg_next(rval, seed);
    let fy = get_fiddle(rval);
    rval = lcg_next(rval, seed);
    let fz = get_fiddle(rval);
    (dz + fz) * (dz + fz) + (dy + fy) * (dy + fy) + (dx + fx) * (dx + fx)
}

/// Fuzzed biome lookup matching vanilla's `BiomeManager.getBiome()`.
///
/// Applies Voronoi-like fuzzing to biome boundaries so they're irregular
/// rather than perfectly grid-aligned at 4-block intervals. When the fuzzed
/// position falls outside the current chunk, reads from neighbor chunk
/// palettes via `neighbor_biomes`.
#[allow(clippy::similar_names, clippy::too_many_arguments)]
fn get_fuzzed_biome(
    sections: &Sections,
    biome_zoom_seed: i64,
    block_x: i32,
    block_y: i32,
    block_z: i32,
    min_y: i32,
    chunk_quart_x: i32,
    chunk_quart_z: i32,
    neighbor_biomes: &dyn Fn(i32, i32, i32) -> u16,
) -> u16 {
    let abs_x = block_x - 2;
    let abs_y = block_y - 2;
    let abs_z = block_z - 2;
    let parent_x = abs_x >> 2;
    let parent_y = abs_y >> 2;
    let parent_z = abs_z >> 2;
    let fract_x = f64::from(abs_x & 3) / 4.0;
    let fract_y = f64::from(abs_y & 3) / 4.0;
    let fract_z = f64::from(abs_z & 3) / 4.0;

    let mut min_i = 0usize;
    let mut min_dist = f64::INFINITY;

    for i in 0..8usize {
        let x_even = (i & 4) == 0;
        let y_even = (i & 2) == 0;
        let z_even = (i & 1) == 0;
        let cx = if x_even { parent_x } else { parent_x + 1 };
        let cy = if y_even { parent_y } else { parent_y + 1 };
        let cz = if z_even { parent_z } else { parent_z + 1 };
        let dx = if x_even { fract_x } else { fract_x - 1.0 };
        let dy = if y_even { fract_y } else { fract_y - 1.0 };
        let dz = if z_even { fract_z } else { fract_z - 1.0 };
        let dist = get_fiddled_distance(biome_zoom_seed, cx, cy, cz, dx, dy, dz);

        if min_dist > dist {
            min_i = i;
            min_dist = dist;
        }
    }

    let biome_qx = if (min_i & 4) == 0 {
        parent_x
    } else {
        parent_x + 1
    };
    let biome_qy = if (min_i & 2) == 0 {
        parent_y
    } else {
        parent_y + 1
    };
    let biome_qz = if (min_i & 1) == 0 {
        parent_z
    } else {
        parent_z + 1
    };

    // Check if the fuzzed position is within the current chunk's quart range
    let in_chunk = biome_qx >= chunk_quart_x
        && biome_qx < chunk_quart_x + 4
        && biome_qz >= chunk_quart_z
        && biome_qz < chunk_quart_z + 4;

    if in_chunk {
        // Read from chunk palette
        let min_qy = min_y >> 2;
        let total_quarts_y = sections.sections.len() * 4;
        let local_qx = (biome_qx - chunk_quart_x) as usize;
        let local_qz = (biome_qz - chunk_quart_z) as usize;
        let qy_in_chunk = (biome_qy - min_qy).clamp(0, total_quarts_y as i32 - 1) as usize;
        let section_idx = qy_in_chunk / 4;
        let local_qy = qy_in_chunk % 4;

        sections.sections[section_idx]
            .read()
            .biomes
            .get(local_qx, local_qy, local_qz)
    } else {
        // Out of chunk bounds — read from neighbor chunk palettes,
        // matching vanilla's WorldGenRegion.getNoiseBiome().
        neighbor_biomes(biome_qx, biome_qy, biome_qz)
    }
}
