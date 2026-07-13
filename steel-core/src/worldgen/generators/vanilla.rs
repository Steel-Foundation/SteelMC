use std::{cell::Cell, marker::PhantomData};

use glam::{DVec3, IVec3};
use rustc_hash::FxHashSet;
use smallvec::SmallVec;
use steel_math::lerp2;
use steel_registry::biome::BiomeRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::carver::ConfiguredCarverKind;
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt, vanilla_biomes};
use steel_utils::random::{
    Random, RandomSource, RandomSplitter, legacy_random::LegacyRandom, xoroshiro::Xoroshiro,
};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, Identifier};
use steel_worldgen::density::{ColumnCache, DimensionNoises, NoiseSettings};
use steel_worldgen::noise_parameters::get_noise_parameters;
use steel_worldgen::surface::{
    SurfaceBiomeProvider, SurfaceConditionNoiseCache, SurfaceRuleContext,
};

use crate::behavior::BlockStateBehaviorExt as _;
use crate::chunk::chunk_access::ChunkAccess;
use crate::chunk::heightmap::{Heightmap, HeightmapType};
use crate::worldgen::carver::{
    CarveRun, CarverBlockIds, CarvingContext, PreliminarySurfaceCorners, SourceChunk, cave,
};
use crate::worldgen::feature::FeatureDecorationRunner;
use crate::worldgen::generator::{ChunkGenerator, worldgen_region_random_from_splitter};
use crate::worldgen::region::WorldGenRegion;
use crate::worldgen::structure::{StructureGenerator, create_structures};
use crate::worldgen::surface::SurfaceSystem;
use steel_worldgen::biomes::BiomeSourceKind;
use steel_worldgen::biomes::obfuscate_biome_seed;
use steel_worldgen::noise::Beardifier;
use steel_worldgen::noise::NoiseChunk;
use steel_worldgen::noise::OreVeinifier;
use steel_worldgen::noise::{Aquifer, AquiferResult, LazyAquifer, preliminary_surface_level};
use steel_worldgen::structure::GenerationContext;

const CARVER_SOURCE_CHUNK_COUNT: usize = 17 * 17;

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
    /// Representative biome for source-carver lookup when every possible
    /// biome from `biome_source` has the same carver list.
    ///
    /// Vanilla still samples each source biome in `apply_carvers`; Steel skips
    /// that sampling only when the source's full possible-biome set proves the
    /// carver list is uniform. If future biome sources can produce mixed
    /// carver lists this remains `None` and the vanilla per-source lookup is
    /// used.
    uniform_carver_biome: Option<BiomeRef>,
    /// Noise generators for this dimension's density functions.
    /// Boxed because noise structs can be large.
    noises: Box<N>,
    /// Seed positional splitter for per-chunk construction of aquifers.
    splitter: RandomSplitter,
    /// Ore vein generator for replacing stone with ore blocks.
    ore_veinifier: Option<OreVeinifier>,
    /// Surface system for biome-specific block replacement.
    surface_system: SurfaceSystem,
    /// Which vanilla surface extension biomes this source can produce.
    surface_extension_biomes: SurfaceExtensionBiomes,
    /// Block state ID for the default block, cached at construction time.
    default_block_id: BlockStateId,
    /// Obfuscated seed for `BiomeManager` biome zoom fuzzing.
    biome_zoom_seed: i64,
    /// World seed as i64 (matching Java's long), used for structures and carver seeding.
    seed: i64,
    /// Shared structure placement/selection engine.
    structure_generator: StructureGenerator,
    /// Cached placed-feature order for biome decoration.
    feature_runner: FeatureDecorationRunner,
    _phantom: PhantomData<N>,
}

#[derive(Clone, Copy)]
struct SurfaceExtensionBiomes {
    eroded_badlands: bool,
    frozen_ocean: bool,
}

impl SurfaceExtensionBiomes {
    fn from_possible(possible_biomes: &FxHashSet<Identifier>) -> Self {
        Self {
            eroded_badlands: possible_biomes.contains(&vanilla_biomes::ERODED_BADLANDS.key),
            frozen_ocean: possible_biomes.contains(&vanilla_biomes::FROZEN_OCEAN.key)
                || possible_biomes.contains(&vanilla_biomes::DEEP_FROZEN_OCEAN.key),
        }
    }

    const fn needs_surface_biome(self) -> bool {
        self.eroded_badlands || self.frozen_ocean
    }
}

impl<N: DimensionNoises> VanillaGenerator<N> {
    /// Creates a new `VanillaGenerator` with the given biome source and seed.
    ///
    /// # Panics
    /// Panics if SHA-256 hash output is shorter than 8 bytes (cannot happen).
    #[must_use]
    pub fn new(biome_source: BiomeSourceKind, seed: u64) -> Self {
        // Nether uses Java's LCG; overworld/end use Xoroshiro.
        let splitter = if N::Settings::LEGACY_RANDOM_SOURCE {
            LegacyRandom::from_seed(seed).next_positional()
        } else {
            Xoroshiro::from_seed(seed).next_positional()
        };
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
            N::surface_gradient_ids(),
            default_block_id,
            N::Settings::SEA_LEVEL,
        );

        let biome_zoom_seed = obfuscate_biome_seed(seed as i64);

        let possible_biome_refs = biome_source.possible_biome_refs();
        let possible_biomes = biome_source.possible_biomes();
        let surface_extension_biomes = SurfaceExtensionBiomes::from_possible(&possible_biomes);
        let structure_generator = StructureGenerator::vanilla(seed as i64, &biome_source);
        let uniform_carver_biome = Self::uniform_carver_biome(&possible_biomes);
        let feature_runner = FeatureDecorationRunner::new(&possible_biome_refs, &REGISTRY);

        Self {
            biome_source,
            uniform_carver_biome,
            noises: Box::new(noises),
            splitter,
            ore_veinifier,
            surface_system,
            surface_extension_biomes,
            default_block_id,
            biome_zoom_seed,
            seed: seed as i64,
            structure_generator,
            feature_runner,
            _phantom: PhantomData,
        }
    }

    fn uniform_carver_biome(possible_biomes: &FxHashSet<Identifier>) -> Option<BiomeRef> {
        let mut possible_biomes = possible_biomes.iter();
        let first_key = possible_biomes.next()?;
        let first = REGISTRY.biomes.by_key(first_key)?;

        possible_biomes
            .all(|key| {
                REGISTRY
                    .biomes
                    .by_key(key)
                    .is_some_and(|biome| biome.carvers == first.carvers)
            })
            .then_some(first)
    }
}

impl<N: DimensionNoises> ChunkGenerator for VanillaGenerator<N> {
    fn min_y(&self) -> i32 {
        N::Settings::MIN_Y
    }

    fn gen_depth(&self) -> i32 {
        N::Settings::HEIGHT
    }

    fn initial_spawn_search_origin(&self) -> steel_utils::BlockPos {
        self.biome_source.initial_spawn_search_origin()
    }

    fn structure_generator(&self) -> Option<&StructureGenerator> {
        Some(&self.structure_generator)
    }

    fn create_structures(&self, chunk: &ChunkAccess) {
        let pos = chunk.pos();
        let chunk_x = pos.0.x;
        let chunk_z = pos.0.y;

        let mut sampler = self.biome_source.chunk_sampler();
        let chunk_min_x = chunk_x * 16;
        let chunk_min_z = chunk_z * 16;

        let mut height_cache = N::ColumnCache::default();
        let sea_level = N::Settings::SEA_LEVEL;

        // No eager `init_grid`: most chunks' structures (mineshaft, village)
        // use their own caches, and the 1–4 column probes of the remainder
        // hit this cache's lazy single-entry mode cheaply. Eager 5×5 quart
        // init cost ~36µs per chunk with no payoff.
        let mut aquifer = LazyAquifer::new(chunk_min_x, chunk_min_z, &self.splitter, &*self.noises);
        let mut surface_y_cache: Option<i32> = None;
        let mut height_cache_grid_ready = false;
        let mut ctx = GenerationContext::<'_, '_, N>::new(
            self.seed,
            chunk_x,
            chunk_z,
            sea_level,
            &self.noises,
            &self.splitter,
            self.structure_generator.template_pools(),
            self.structure_generator.templates(),
            &mut sampler,
            &mut height_cache,
            &mut aquifer,
            &mut surface_y_cache,
            &mut height_cache_grid_ready,
        );

        create_structures(&self.structure_generator, chunk, &mut ctx);
    }

    fn create_biomes(&self, chunk: &ChunkAccess) {
        let pos = chunk.pos();
        let min_y = chunk.min_y();
        let section_count = chunk.sections().sections.len();

        let chunk_x = pos.0.x;
        let chunk_z = pos.0.y;

        let mut sampler = self.biome_source.chunk_sampler();
        // Pre-compute the flat (xz-only) climate-noise grid for this chunk so the
        // per-cell sampling below does O(1) column lookups instead of recomputing
        // the flat noise for all 1536 cells (the noise stage's `fill_from_noise`
        // already does this). Values are bit-identical — same functions, same quart
        // coordinates — so biome selection is unchanged.
        sampler.init_grid(chunk_x * 16, chunk_z * 16);

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
                        let biome_id = biome.id() as u16;

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

    fn fill_from_noise(&self, chunk: &ChunkAccess, beardifier: Option<&Beardifier>) {
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

        // Collect writes per (x,z) column and flush in batch to avoid per-block
        // write lock acquisition on sections.
        let mut pending_writes: Vec<(usize, usize, usize, BlockStateId)> = Vec::new();
        let mut prev_x: usize = usize::MAX;
        let mut prev_z: usize = usize::MAX;
        let mut ocean_floor_wg =
            Heightmap::new(HeightmapType::OceanFloorWg, min_y, N::Settings::HEIGHT);
        let mut world_surface_wg =
            Heightmap::new(HeightmapType::WorldSurfaceWg, min_y, N::Settings::HEIGHT);

        noise_chunk.fill(
            noises,
            &mut column_cache,
            beardifier,
            |local_x, world_y, local_z, density, interpolated, cache| {
                // Flush when we move to a new column
                if local_x != prev_x || local_z != prev_z {
                    if !pending_writes.is_empty() {
                        chunk.write_block_batch_for_generation(&pending_writes);
                        pending_writes.clear();
                    }
                    prev_x = local_x;
                    prev_z = local_z;
                }

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
                        pending_writes.push((local_x, relative_y, local_z, block));
                        ocean_floor_wg.update_for_initial_fill(local_x, world_y, local_z, block);
                        world_surface_wg.update_for_initial_fill(local_x, world_y, local_z, block);
                    }
                    AquiferResult::Fluid(id) => {
                        pending_writes.push((local_x, relative_y, local_z, id));
                        ocean_floor_wg.update_for_initial_fill(local_x, world_y, local_z, id);
                        world_surface_wg.update_for_initial_fill(local_x, world_y, local_z, id);
                        if aquifer.should_schedule_fluid_update() && id.has_fluid() {
                            chunk.mark_pos_for_postprocessing(BlockPos::new(
                                world_x, world_y, world_z,
                            ));
                        }
                    }
                    AquiferResult::Air => {}
                }
            },
        );

        // Flush remaining writes
        if !pending_writes.is_empty() {
            chunk.write_block_batch_for_generation(&pending_writes);
        }

        let ChunkAccess::Proto(proto) = chunk else {
            return;
        };
        let mut heightmaps = proto.heightmaps.write();
        heightmaps.replace(ocean_floor_wg);
        heightmaps.replace(world_surface_wg);
    }

    #[expect(clippy::too_many_lines, reason = "splitting would hurt readability")]
    fn build_surface(&self, chunk: &ChunkAccess, neighbor_biomes: &dyn Fn(IVec3) -> u16) {
        let min_y = N::Settings::MIN_Y;
        let pos = chunk.pos();
        let chunk_min_x = pos.0.x * 16;
        let chunk_min_z = pos.0.y * 16;
        let default_block_id = self.default_block_id;
        let noises = &*self.noises;
        let surface_rule_block_states = N::surface_rule_block_states();
        let surface_rule_uses_biome = N::surface_rule_uses_biome();
        let surface_rule_uses_preliminary_surface = N::surface_rule_uses_preliminary_surface();
        let surface_rule_uses_surface_secondary = N::surface_rule_uses_surface_secondary();
        let surface_rule_uses_steep = N::surface_rule_uses_steep();
        let lazy_surface_rule_biome =
            surface_rule_uses_biome && surface_rule_uses_preliminary_surface;
        let surface_needs_min_surface_level =
            surface_rule_uses_preliminary_surface || self.surface_extension_biomes.frozen_ocean;
        let surface_needs_biomes =
            surface_rule_uses_biome || self.surface_extension_biomes.needs_surface_biome();
        let chunk_quart_x = pos.0.x * 4;
        let chunk_quart_z = pos.0.y * 4;

        chunk.prime_heightmaps(&[HeightmapType::WorldSurfaceWg]);

        // Pre-compute preliminary surface corners only for rules/extensions that read them.
        let preliminary_surface_corners = surface_needs_min_surface_level.then(|| {
            let mut psl_cache = N::ColumnCache::default();
            let p00 =
                preliminary_surface_level::<N>(noises, &mut psl_cache, chunk_min_x, chunk_min_z);
            let p10 = preliminary_surface_level::<N>(
                noises,
                &mut psl_cache,
                chunk_min_x + 16,
                chunk_min_z,
            );
            let p01 = preliminary_surface_level::<N>(
                noises,
                &mut psl_cache,
                chunk_min_x,
                chunk_min_z + 16,
            );
            let p11 = preliminary_surface_level::<N>(
                noises,
                &mut psl_cache,
                chunk_min_x + 16,
                chunk_min_z + 16,
            );
            (p00, p10, p01, p11)
        });

        let eroded_badlands_id = (*vanilla_biomes::ERODED_BADLANDS).id() as u16;
        let frozen_ocean_id = (*vanilla_biomes::FROZEN_OCEAN).id() as u16;
        let deep_frozen_ocean_id = (*vanilla_biomes::DEEP_FROZEN_OCEAN).id() as u16;

        // Pre-extract biome palette values only if surface rules/extensions need them.
        let biome_data = surface_needs_biomes.then(|| chunk.sections().read_all_biomes());
        let section_count = chunk.sections().sections.len();

        let mut pending_writes: Vec<(usize, BlockStateId)> = Vec::new();
        let mut column_buf: Vec<BlockStateId> = Vec::new();
        let condition_noise_values = N::surface_noise_ids()
            .iter()
            .map(|_| Cell::new(0.0))
            .collect::<Vec<_>>();
        let condition_noise_initialized = N::surface_noise_ids()
            .iter()
            .map(|_| Cell::new(false))
            .collect::<Vec<_>>();
        let condition_noise_cache =
            SurfaceConditionNoiseCache::new(&condition_noise_values, &condition_noise_initialized);

        for local_x in 0..16usize {
            for local_z in 0..16usize {
                let block_x = chunk_min_x + local_x as i32;
                let block_z = chunk_min_z + local_z as i32;

                // Start scanning from one above the highest non-air block
                let mut start_height =
                    chunk.height_at(HeightmapType::WorldSurfaceWg, local_x, local_z);

                // Column-local Voronoi cache for fuzzed biome lookups.
                let mut biome_col = biome_data.as_deref().map(|biome_data| {
                    FuzzedBiomeColumn::new(
                        biome_data,
                        section_count,
                        self.biome_zoom_seed,
                        block_x,
                        block_z,
                        min_y,
                        chunk_quart_x,
                        chunk_quart_z,
                        neighbor_biomes,
                    )
                });

                // Eroded badlands extension: add terracotta pillars above surface
                let surface_biome_id = if self.surface_extension_biomes.needs_surface_biome() {
                    biome_col
                        .as_mut()
                        .map(|biome_col| biome_col.get(start_height))
                } else {
                    None
                };
                if self.surface_extension_biomes.eroded_badlands
                    && surface_biome_id == Some(eroded_badlands_id)
                {
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

                // Snapshot the column once — avoids per-block section locking in the Y scan.
                // Taken after eroded_badlands_extension which may write blocks above the surface.
                chunk
                    .sections()
                    .read_column_into(local_x, local_z, &mut column_buf);

                // Surface depth for this column
                let surface_depth = self.surface_system.get_surface_depth(block_x, block_z);

                let surface_secondary = if surface_rule_uses_surface_secondary {
                    self.surface_system.get_surface_secondary(block_x, block_z)
                } else {
                    0.0
                };
                condition_noise_cache.reset();

                let min_surface_level =
                    if let Some((p00, p10, p01, p11)) = preliminary_surface_corners {
                        // Vanilla: (float)(blockX & 15) / 16.0F — exact for 0-15.
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
                        interp.floor() as i32 + surface_depth - 8
                    } else {
                        0
                    };

                // Steep condition: vanilla only checks south >= north + 4 and
                // west >= east + 4 (asymmetric, not absolute difference).
                let steep = surface_rule_uses_steep && {
                    let z_north = local_z.saturating_sub(1);
                    let z_south = (local_z + 1).min(15);
                    let h_north =
                        chunk.height_at(HeightmapType::WorldSurfaceWg, local_x, z_north) - 1;
                    let h_south =
                        chunk.height_at(HeightmapType::WorldSurfaceWg, local_x, z_south) - 1;
                    if h_south >= h_north + 4 {
                        true
                    } else {
                        let x_west = local_x.saturating_sub(1);
                        let x_east = (local_x + 1).min(15);
                        let h_west =
                            chunk.height_at(HeightmapType::WorldSurfaceWg, x_west, local_z) - 1;
                        let h_east =
                            chunk.height_at(HeightmapType::WorldSurfaceWg, x_east, local_z) - 1;
                        h_west >= h_east + 4
                    }
                };

                let mut stone_depth_above: i32 = 0;
                let mut water_height: i32 = i32::MIN;
                let mut next_ceiling_stone_y: i32 = i32::MAX;
                pending_writes.clear();

                for y in (min_y..=start_height).rev() {
                    let relative_y = (y - min_y) as usize;
                    let state = column_buf[relative_y];

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
                            let la_state = column_buf[la_rel];
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
                        let eager_biome_id = if surface_rule_uses_biome && !lazy_surface_rule_biome
                        {
                            biome_col.as_mut().map(|biome_col| biome_col.get(y))
                        } else {
                            None
                        };
                        let biome_provider = if lazy_surface_rule_biome {
                            biome_col
                                .as_mut()
                                .map(|biome_col| biome_col as &mut dyn SurfaceBiomeProvider)
                        } else {
                            None
                        };

                        let mut ctx = SurfaceRuleContext::new(
                            block_x,
                            block_z,
                            surface_depth,
                            surface_secondary,
                            min_surface_level,
                            steep,
                            y,
                            stone_depth_above,
                            stone_depth_below,
                            water_height,
                            eager_biome_id,
                            biome_provider,
                            &self.surface_system,
                            &condition_noise_cache,
                            surface_rule_block_states,
                        );

                        let rule_result = N::try_apply_surface_rule(&mut ctx);

                        if let Some(new_block) = rule_result {
                            pending_writes.push((relative_y, new_block));
                        }
                    }
                }

                // Flush batched writes — holds each section's write guard once
                if !pending_writes.is_empty() {
                    chunk.write_column_blocks_for_generation(local_x, local_z, &pending_writes);
                    for &(relative_y, state) in &pending_writes {
                        column_buf[relative_y] = state;
                    }
                    chunk.update_heightmaps_after_direct_column_writes(
                        local_x,
                        local_z,
                        &pending_writes,
                    );
                    chunk.mark_dirty();
                }

                // Frozen ocean iceberg extension: add packed ice and snow
                if self.surface_extension_biomes.frozen_ocean
                    && let Some(surface_biome_id) = surface_biome_id
                        .filter(|id| *id == frozen_ocean_id || *id == deep_frozen_ocean_id)
                {
                    pending_writes.clear();
                    self.surface_system.collect_frozen_ocean_extension_writes(
                        surface_biome_id,
                        block_x,
                        block_z,
                        start_height,
                        min_surface_level,
                        min_y,
                        &column_buf,
                        &mut pending_writes,
                    );
                    if !pending_writes.is_empty() {
                        chunk.write_column_blocks_for_generation(local_x, local_z, &pending_writes);
                        chunk.update_heightmaps_after_direct_column_writes(
                            local_x,
                            local_z,
                            &pending_writes,
                        );
                        chunk.mark_dirty();
                    }
                }
            }
        }
    }

    #[expect(clippy::too_many_lines, reason = "matches vanilla carver setup flow")]
    fn apply_carvers(&self, chunk: &ChunkAccess) {
        // Carvers only run on proto chunks.
        let ChunkAccess::Proto(proto) = chunk else {
            return;
        };

        if self
            .uniform_carver_biome
            .is_some_and(|biome| biome.carvers.is_empty())
        {
            return;
        }

        chunk.prime_heightmaps(&[HeightmapType::WorldSurfaceWg]);

        let pos = chunk.pos();
        let chunk_min_x = pos.0.x * 16;
        let chunk_min_z = pos.0.y * 16;
        let min_y = N::Settings::MIN_Y;
        let height = N::Settings::HEIGHT;
        let noises = &*self.noises;

        // Fresh aquifer (vanilla caches NoiseChunk across stages; see TODO on
        // ProtoChunk::carving_mask for why we rebuild instead).
        let mut column_cache = N::ColumnCache::default();
        if N::Settings::AQUIFERS_ENABLED {
            column_cache.init_grid(chunk_min_x, chunk_min_z, noises);
        }
        let aquifer = Aquifer::<N>::new(
            chunk_min_x,
            chunk_min_z,
            min_y,
            height,
            &self.splitter,
            noises,
            column_cache,
        );

        // Preliminary surface level at the chunk's 4 corners — used by
        // top_material min_surface_level interpolation.
        let mut psl_cache = N::ColumnCache::default();
        let psl_corners = PreliminarySurfaceCorners {
            nw: preliminary_surface_level::<N>(noises, &mut psl_cache, chunk_min_x, chunk_min_z),
            ne: preliminary_surface_level::<N>(
                noises,
                &mut psl_cache,
                chunk_min_x + 16,
                chunk_min_z,
            ),
            sw: preliminary_surface_level::<N>(
                noises,
                &mut psl_cache,
                chunk_min_x,
                chunk_min_z + 16,
            ),
            se: preliminary_surface_level::<N>(
                noises,
                &mut psl_cache,
                chunk_min_x + 16,
                chunk_min_z + 16,
            ),
        };

        let mut ctx = CarvingContext {
            min_y,
            gen_depth: height,
            surface_system: &self.surface_system,
            aquifer,
            default_block_id: self.default_block_id,
            psl_corners,
            chunk_min_x,
            chunk_min_z,
        };

        let ids = CarverBlockIds::load();

        // Pre-fetch the 17×17 source-chunk carver lists. Done up front so we
        // can later close over `biome_sampler` mutably inside `biome_getter`.
        // Vanilla samples every source biome here; when this generator's full
        // possible-biome set has a uniform carver list, the representative
        // biome gives the same carver keys without 289 climate lookups.
        let mut biome_sampler = self.biome_source.chunk_sampler();
        let mut source_biomes: SmallVec<[SourceChunk; CARVER_SOURCE_CHUNK_COUNT]> = SmallVec::new();
        for dx in -8i32..=8 {
            for dz in -8i32..=8 {
                let sx = pos.0.x + dx;
                let sz = pos.0.y + dz;
                let biome = if let Some(biome) = self.uniform_carver_biome {
                    biome
                } else {
                    let qx = (sx * 16) >> 2;
                    let qz = (sz * 16) >> 2;
                    biome_sampler.sample(qx, 0, qz)
                };
                source_biomes.push(SourceChunk {
                    pos: ChunkPos::new(sx, sz),
                    biome,
                });
            }
        }

        // Grab (and lazily create) the carving mask on the proto chunk.
        let mut mask_guard = proto.get_or_create_carving_mask();
        let mask = &mut *mask_guard;

        // `WorldgenRandom(LegacyRandomSource(generateUniqueSeed()))` — initial
        // seed is irrelevant; every carver overwrites it via
        // `set_large_feature_seed` before its probability check.
        let mut random = LegacyRandom::from_seed(0);
        let seed_i64 = self.seed;

        let biome_zoom_seed = self.biome_zoom_seed;
        // BiomeManager-fuzzed lookup — matches vanilla's `BiomeManager.getBiome`
        // used by the carver's top-material path. An unfuzzed quart lookup
        // would mismatch vanilla at quart-cell boundaries.
        let mut biome_getter = |pos: BlockPos| -> u16 {
            fuzzed_biome_at_block(biome_zoom_seed, pos, |q_pos| {
                biome_sampler.sample(q_pos.x, q_pos.y, q_pos.z).id() as u16
            })
        };

        let mut run = CarveRun {
            ctx: &mut ctx,
            noises,
            chunk,
            chunk_min_x,
            chunk_min_z,
            biome_getter: &mut biome_getter,
            mask,
            ids,
        };

        run.run_all(&source_biomes, seed_i64, &mut random);
    }

    fn create_worldgen_region_random(&self, _world_seed: i64, center: ChunkPos) -> RandomSource {
        worldgen_region_random_from_splitter(&self.splitter, center)
    }

    fn apply_biome_decorations(&self, region: &mut WorldGenRegion<'_>) {
        self.feature_runner
            .decorate(region, &REGISTRY, self.seed, self.biome_zoom_seed);
    }
}

impl<N, F> CarveRun<'_, '_, N, F>
where
    N: DimensionNoises,
    F: FnMut(BlockPos) -> u16,
{
    /// Drive the 17×17 source-chunk carver loop. Each carver in each source
    /// biome is seeded via `set_large_feature_seed`, probability-checked,
    /// then dispatched to the appropriate `carve_*` method.
    fn run_all(&mut self, source_biomes: &[SourceChunk], seed_i64: i64, random: &mut LegacyRandom) {
        for source in source_biomes {
            for (index, carver_key) in source.biome.carvers.iter().enumerate() {
                let Some(carver) = REGISTRY.configured_carvers.by_key(carver_key) else {
                    panic!(
                        "biome {} references unknown configured carver {}",
                        source.biome.key, carver_key
                    );
                };
                let index_i64 = index as i64;
                random.set_large_feature_seed(
                    seed_i64.wrapping_add(index_i64),
                    source.pos.0.x,
                    source.pos.0.y,
                );

                let probability = carver.base().probability;
                if random.next_f32() > probability {
                    continue;
                }

                match &carver.kind {
                    ConfiguredCarverKind::Cave(cfg) => {
                        self.carve_cave(cfg, cave::CaveKind::Overworld, source.pos, random);
                    }
                    ConfiguredCarverKind::NetherCave(cfg) => {
                        self.carve_cave(cfg, cave::CaveKind::Nether, source.pos, random);
                    }
                    ConfiguredCarverKind::Canyon(cfg) => {
                        self.carve_canyon(cfg, source.pos, random);
                    }
                }
            }
        }
    }
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

/// Single-shot fuzzed biome lookup at a block position. Matches vanilla's
/// `BiomeManager.getBiome(BlockPos)`: the block is shifted by `-2`, snapped to
/// the enclosing quart cell, and the winning biome is chosen from the 8
/// corners of that cell by `get_fiddle`-perturbed squared distance.
///
/// `quart_biome` returns the unfuzzed biome at a quart-coordinate — typically
/// `biome_sampler.sample(qx, qy, qz).id()`.
///
/// Used by carver top-material lookups where a simple unfuzzed lookup would
/// differ from vanilla at the quart-cell boundaries.
pub(crate) fn fuzzed_biome_at_block<F: FnMut(IVec3) -> u16>(
    biome_zoom_seed: i64,
    pos: BlockPos,
    mut quart_biome: F,
) -> u16 {
    let abs = pos.0 - IVec3::splat(2);
    let parent = IVec3::new(abs.x >> 2, abs.y >> 2, abs.z >> 2);
    let fract = DVec3::new(
        f64::from(abs.x & 3),
        f64::from(abs.y & 3),
        f64::from(abs.z & 3),
    ) / 4.0;

    let mut min_i = 0usize;
    let mut min_dist = f64::INFINITY;

    for i in 0..8usize {
        let x_even = (i & 4) == 0;
        let y_even = (i & 2) == 0;
        let z_even = (i & 1) == 0;
        let cx = if x_even { parent.x } else { parent.x + 1 };
        let cy = if y_even { parent.y } else { parent.y + 1 };
        let cz = if z_even { parent.z } else { parent.z + 1 };
        let dx = if x_even { fract.x } else { fract.x - 1.0 };
        let dy = if y_even { fract.y } else { fract.y - 1.0 };
        let dz = if z_even { fract.z } else { fract.z - 1.0 };

        // BiomeManager.getFiddledDistance — identical sequence to
        // FuzzedBiomeColumn::compute_cy_group but without the column cache.
        let mut rval = lcg_next(biome_zoom_seed, i64::from(cx));
        rval = lcg_next(rval, i64::from(cy));
        rval = lcg_next(rval, i64::from(cz));
        rval = lcg_next(rval, i64::from(cx));
        rval = lcg_next(rval, i64::from(cy));
        rval = lcg_next(rval, i64::from(cz));
        let fx = get_fiddle(rval);
        rval = lcg_next(rval, biome_zoom_seed);
        let fy = get_fiddle(rval);
        rval = lcg_next(rval, biome_zoom_seed);
        let fz = get_fiddle(rval);

        let dist = (dx + fx).powi(2) + (dy + fy).powi(2) + (dz + fz).powi(2);
        if min_dist > dist {
            min_i = i;
            min_dist = dist;
        }
    }

    let b = IVec3::new(
        if (min_i & 4) == 0 {
            parent.x
        } else {
            parent.x + 1
        },
        if (min_i & 2) == 0 {
            parent.y
        } else {
            parent.y + 1
        },
        if (min_i & 1) == 0 {
            parent.z
        } else {
            parent.z + 1
        },
    );
    quart_biome(b)
}

/// Column-local cache for fuzzed biome lookups (vanilla `BiomeManager.getBiome()`).
///
/// Within a column, `parent_x`, `parent_z`, `fract_x`, `fract_z` are constant.
/// The 8 Voronoi candidate fiddle values (computed via 8 serial LCG calls each)
/// only change when `parent_y` changes (every 4 blocks). This cache precomputes
/// the fiddle values and X/Z distance components per `parent_y` group, reducing
/// per-block work to 8 additions + 8 multiplies + 8 comparisons.
struct FuzzedBiomeColumn<'a> {
    biome_data: &'a [u16],
    section_count: usize,
    biome_zoom_seed: i64,
    parent_x: i32,
    parent_z: i32,
    fract_x: f64,
    fract_z: f64,
    min_y: i32,
    chunk_quart_x: i32,
    chunk_quart_z: i32,
    neighbor_biomes: &'a dyn Fn(IVec3) -> u16,
    cached_parent_y: i32,
    /// Per-candidate cached values: (`fy`, `xz_partial_distance`).
    candidates: [(f64, f64); 8],
    /// Precomputed `lcg_next(seed, parent_x)` and `lcg_next(seed, parent_x + 1)`.
    rval_after_cx: [i64; 2],
}

impl<'a> FuzzedBiomeColumn<'a> {
    #[expect(
        clippy::too_many_arguments,
        reason = "matches vanilla BiomeManager.getBiome signature"
    )]
    fn new(
        biome_data: &'a [u16],
        section_count: usize,
        biome_zoom_seed: i64,
        block_x: i32,
        block_z: i32,
        min_y: i32,
        chunk_quart_x: i32,
        chunk_quart_z: i32,
        neighbor_biomes: &'a dyn Fn(IVec3) -> u16,
    ) -> Self {
        let abs_x = block_x - 2;
        let abs_z = block_z - 2;
        let parent_x = abs_x >> 2;
        let parent_z = abs_z >> 2;
        Self {
            biome_data,
            section_count,
            biome_zoom_seed,
            parent_x,
            parent_z,
            fract_x: f64::from(abs_x & 3) / 4.0,
            fract_z: f64::from(abs_z & 3) / 4.0,
            min_y,
            chunk_quart_x,
            chunk_quart_z,
            neighbor_biomes,
            cached_parent_y: i32::MIN,
            candidates: [(0.0, 0.0); 8],
            rval_after_cx: [
                lcg_next(biome_zoom_seed, i64::from(parent_x)),
                lcg_next(biome_zoom_seed, i64::from(parent_x + 1)),
            ],
        }
    }

    /// Compute candidates for a given `cy`, writing to either the low (bit1=0)
    /// or high (bit1=1) slots. Shares the `lcg_next(seed, cx)` precomputation
    /// and the `lcg_next(_, cy)` step within each cx group.
    #[inline]
    fn compute_cy_group(&mut self, cy: i32, high: bool) {
        let base_idx = if high { 2 } else { 0 };
        for cx_idx in 0..2usize {
            let cx = self.parent_x + cx_idx as i32;
            let dx = if cx_idx == 0 {
                self.fract_x
            } else {
                self.fract_x - 1.0
            };
            let rval_cy = lcg_next(self.rval_after_cx[cx_idx], i64::from(cy));
            for cz_off in 0..2usize {
                let cz = self.parent_z + cz_off as i32;
                let dz = if cz_off == 0 {
                    self.fract_z
                } else {
                    self.fract_z - 1.0
                };

                let mut rval = lcg_next(rval_cy, i64::from(cz));
                rval = lcg_next(rval, i64::from(cx));
                rval = lcg_next(rval, i64::from(cy));
                rval = lcg_next(rval, i64::from(cz));
                let fx = get_fiddle(rval);
                rval = lcg_next(rval, self.biome_zoom_seed);
                let fy = get_fiddle(rval);
                rval = lcg_next(rval, self.biome_zoom_seed);
                let fz = get_fiddle(rval);

                let xz_partial = (dx + fx) * (dx + fx) + (dz + fz) * (dz + fz);
                self.candidates[cx_idx * 4 + base_idx + cz_off] = (fy, xz_partial);
            }
        }
    }

    /// Recompute the 8 candidate fiddle values and X/Z distance for a new `parent_y`.
    ///
    /// When scanning downward (`parent_y` decreases by 1), the old low-cy candidates
    /// (`cy=old_parent_y`) match the new high-cy slots (`cy=new_parent_y+1`), so only
    /// the 4 new low-cy candidates need fresh LCG computation.
    fn recompute_candidates(&mut self, parent_y: i32) {
        if self.cached_parent_y != i32::MIN && parent_y == self.cached_parent_y - 1 {
            // Reuse: old low-cy group → new high-cy group
            self.candidates[2] = self.candidates[0];
            self.candidates[3] = self.candidates[1];
            self.candidates[6] = self.candidates[4];
            self.candidates[7] = self.candidates[5];
            self.compute_cy_group(parent_y, false);
        } else {
            self.compute_cy_group(parent_y, false);
            self.compute_cy_group(parent_y + 1, true);
        }
        self.cached_parent_y = parent_y;
    }

    /// Fuzzed biome lookup for a given `block_y`.
    #[expect(
        clippy::similar_names,
        reason = "matches vanilla variable names: fract_x/y/z, parent_x/y/z"
    )]
    #[inline]
    fn get(&mut self, block_y: i32) -> u16 {
        let abs_y = block_y - 2;
        let parent_y = abs_y >> 2;
        let fract_y = f64::from(abs_y & 3) / 4.0;

        if parent_y != self.cached_parent_y {
            self.recompute_candidates(parent_y);
        }

        let mut min_i = 0usize;
        let mut min_dist = f64::INFINITY;
        for i in 0..8usize {
            let (fy, xz_partial) = self.candidates[i];
            let dy = if (i & 2) == 0 { fract_y } else { fract_y - 1.0 };
            let dist = xz_partial + (dy + fy) * (dy + fy);
            if min_dist > dist {
                min_i = i;
                min_dist = dist;
            }
        }

        let biome_quart = IVec3::new(
            if (min_i & 4) == 0 {
                self.parent_x
            } else {
                self.parent_x + 1
            },
            if (min_i & 2) == 0 {
                parent_y
            } else {
                parent_y + 1
            },
            if (min_i & 1) == 0 {
                self.parent_z
            } else {
                self.parent_z + 1
            },
        );

        let in_chunk = biome_quart.x >= self.chunk_quart_x
            && biome_quart.x < self.chunk_quart_x + 4
            && biome_quart.z >= self.chunk_quart_z
            && biome_quart.z < self.chunk_quart_z + 4;

        if in_chunk {
            let min_qy = self.min_y >> 2;
            let total_quarts_y = self.section_count * 4;
            let local_qx = (biome_quart.x - self.chunk_quart_x) as usize;
            let local_qz = (biome_quart.z - self.chunk_quart_z) as usize;
            let qy_in_chunk = (biome_quart.y - min_qy).clamp(0, total_quarts_y as i32 - 1) as usize;
            let section_idx = qy_in_chunk / 4;
            let local_qy = qy_in_chunk % 4;
            self.biome_data[section_idx * 64 + local_qy * 16 + local_qz * 4 + local_qx]
        } else {
            (self.neighbor_biomes)(biome_quart)
        }
    }
}

impl SurfaceBiomeProvider for FuzzedBiomeColumn<'_> {
    #[inline]
    fn biome_id(&mut self, block_y: i32) -> u16 {
        self.get(block_y)
    }
}
