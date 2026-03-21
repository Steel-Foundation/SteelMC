use std::marker::PhantomData;

use sha2::{Digest, Sha256};
use steel_registry::RegistryEntry;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::noise_parameters::get_noise_parameters;
use steel_registry::vanilla_biomes;
use steel_utils::density::{ColumnCache, DimensionNoises, NoiseSettings};
use steel_utils::math::noise_math::lerp2;
use steel_utils::random::{
    Random, RandomSplitter, legacy_random::LegacyRandom, xoroshiro::Xoroshiro,
};
use steel_utils::surface::SurfaceRuleContext;
use steel_utils::{BlockStateId, BoundingBox, Identifier, Rotation};

use crate::chunk::aquifer::{Aquifer, AquiferResult, preliminary_surface_level};
use crate::chunk::beardifier::Beardifier;
use crate::chunk::chunk_access::ChunkAccess;
use crate::chunk::chunk_generator::ChunkGenerator;
use crate::chunk::heightmap::HeightmapType;
use crate::chunk::noise_chunk::NoiseChunk;
use crate::chunk::ore_veinifier::OreVeinifier;
use crate::chunk::surface_system::SurfaceSystem;
use crate::world::structure::mineshaft::{self, MineshaftType};
use crate::world::structure::placement::{
    PlacementKind, StructureSelectionEntry, StructureSet, generate_ring_positions,
    load_vanilla_structure_sets,
};
use crate::world::structure::ruined_portal;
use crate::world::structure::{StructurePiece, StructureStart};
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
    /// World seed as i64 (matching Java's long), used for structure placement.
    seed: i64,
    /// Loaded structure sets for placement checks.
    structure_sets: Vec<(Identifier, StructureSet)>,
    /// Pre-computed ring positions for `ConcentricRings` placements, keyed by set identifier.
    ring_positions: Vec<(Identifier, Vec<steel_utils::ChunkPos>)>,
    /// Template pool registry for jigsaw assembly.
    template_pools:
        rustc_hash::FxHashMap<Identifier, steel_registry::template_pool::TemplatePoolData>,
    /// Structure template data (size + jigsaw blocks) for jigsaw assembly.
    templates: rustc_hash::FxHashMap<Identifier, steel_registry::template_pool::TemplateData>,
    _phantom: PhantomData<N>,
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

        let structure_sets = load_vanilla_structure_sets();

        // Pre-compute ring positions for ConcentricRings placements (e.g., strongholds).
        // Positions are snapped to preferred biomes via findBiomeHorizontal (search radius 112).
        let mut ring_positions = Vec::new();
        for (key, set) in &structure_sets {
            if let PlacementKind::ConcentricRings {
                distance,
                spread,
                count,
                preferred_biomes,
            } = &set.placement.kind
            {
                let biomes = preferred_biomes;
                let mut snap =
                    |block_x: i32, block_z: i32, rng: &mut LegacyRandom| -> Option<(i32, i32)> {
                        biome_source.find_biome_horizontal(
                            block_x,
                            block_z,
                            112,
                            &|biome| biomes.contains(&biome.key),
                            rng,
                        )
                    };
                let positions = generate_ring_positions(
                    seed as i64,
                    *distance,
                    *spread,
                    *count,
                    Some(&mut snap),
                );
                ring_positions.push((key.clone(), positions));
            }
        }

        // Load template pools and structure templates for jigsaw assembly.
        let template_pools: rustc_hash::FxHashMap<_, _> =
            steel_registry::vanilla_template_pools::vanilla_template_pools()
                .into_iter()
                .map(|p| (p.key.clone(), p))
                .collect();
        let templates: rustc_hash::FxHashMap<_, _> =
            steel_registry::vanilla_template_pools::vanilla_templates()
                .into_iter()
                .collect();

        Self {
            biome_source,
            noises: Box::new(noises),
            splitter,
            ore_veinifier,
            surface_system,
            default_block_id,
            biome_zoom_seed,
            seed: seed as i64,
            structure_sets,
            ring_positions,
            template_pools,
            templates,
            _phantom: PhantomData,
        }
    }
}

/// Evaluates density using trilinear interpolation from cell corners,
/// matching vanilla's `NoiseChunk` behavior.
/// Vanilla inflates the structure BB by 12 when `terrain_adaptation != NONE`.
/// Returns the inflate value for reference intersection checks.
fn terrain_adapt_inflate(id: &Identifier) -> i32 {
    match id.path.as_ref() {
        "stronghold" | "trail_ruins" | "ancient_city" | "nether_fossil" | "pillager_outpost"
        | "trial_chambers" | "village_desert" | "village_plains" | "village_savanna"
        | "village_snowy" | "village_taiga" => 12,
        _ => 0,
    }
}

/// Matches vanilla's `iterateNoiseColumn`: iterates by Y cells, evaluating
/// inner density functions at 8 cell corners, trilinearly interpolating each
/// channel independently, then applying outer operations (squeeze, min, etc.)
/// per-block via `combine_interpolated`.
///
/// Returns getBaseHeight (= getFirstFreeHeight = first Y above surface).
fn iterate_noise_column_with_aquifer<N: DimensionNoises>(
    cache: &mut N::ColumnCache,
    noises: &N,
    aquifer: &mut Aquifer<N>,
    block_x: i32,
    block_z: i32,
    ocean_floor: bool,
) -> i32 {
    let cell_w = N::Settings::CELL_WIDTH;
    let cell_h = N::Settings::CELL_HEIGHT;
    let min_y = N::Settings::MIN_Y;
    let height = N::Settings::HEIGHT;
    let cell_min_y = min_y.div_euclid(cell_h);
    let cell_count_y = height.div_euclid(cell_h);

    let cell_x = block_x.div_euclid(cell_w);
    let cell_z = block_z.div_euclid(cell_w);
    let factor_x = f64::from(block_x.rem_euclid(cell_w)) / f64::from(cell_w);
    let factor_z = f64::from(block_z.rem_euclid(cell_w)) / f64::from(cell_w);
    let x0 = cell_x * cell_w;
    let x1 = x0 + cell_w;
    let z0 = cell_z * cell_w;
    let z1 = z0 + cell_w;

    let interp_count = N::interpolated_count();

    // Corner channel buffers for 8 cell corners
    const MAX_INTERP: usize = 16;
    let mut c000 = [0.0f64; MAX_INTERP];
    let mut c100 = [0.0f64; MAX_INTERP];
    let mut c010 = [0.0f64; MAX_INTERP];
    let mut c110 = [0.0f64; MAX_INTERP];
    let mut c001 = [0.0f64; MAX_INTERP];
    let mut c101 = [0.0f64; MAX_INTERP];
    let mut c011 = [0.0f64; MAX_INTERP];
    let mut c111 = [0.0f64; MAX_INTERP];
    let mut interpolated = [0.0f64; MAX_INTERP];

    macro_rules! fill {
        ($out:expr, $ex:expr, $ey:expr, $ez:expr) => {{
            cache.ensure($ex, $ez, noises);
            noises.fill_cell_corner_densities(
                &mut *cache,
                $ex,
                $ey,
                $ez,
                &mut $out[..interp_count],
            );
        }};
    }

    use steel_utils::math::noise_math::lerp;

    // Iterate Y cells from top to bottom (matching vanilla's loop)
    for cell_y_idx in (0..cell_count_y).rev() {
        let y0 = (cell_min_y + cell_y_idx) * cell_h;
        let y1 = y0 + cell_h;

        // Evaluate inner functions at 8 cell corners (all channels)
        fill!(c000, x0, y0, z0);
        fill!(c100, x1, y0, z0);
        fill!(c010, x0, y1, z0);
        fill!(c110, x1, y1, z0);
        fill!(c001, x0, y0, z1);
        fill!(c101, x1, y0, z1);
        fill!(c011, x0, y1, z1);
        fill!(c111, x1, y1, z1);

        // Iterate Y within cell from top to bottom
        for y_in_cell in (0..cell_h).rev() {
            let pos_y = (cell_min_y + cell_y_idx) * cell_h + y_in_cell;
            let factor_y = f64::from(y_in_cell) / f64::from(cell_h);

            // Trilinearly interpolate each channel independently
            for ch in 0..interp_count {
                let d00 = lerp(factor_y, c000[ch], c010[ch]);
                let d10 = lerp(factor_y, c100[ch], c110[ch]);
                let d01 = lerp(factor_y, c001[ch], c011[ch]);
                let d11 = lerp(factor_y, c101[ch], c111[ch]);
                let d0 = lerp(factor_x, d00, d10);
                let d1 = lerp(factor_x, d01, d11);
                interpolated[ch] = lerp(factor_z, d0, d1);
            }

            // Apply outer operations (squeeze, min, etc.) per-block
            let density = noises.combine_interpolated(
                &mut *cache,
                &interpolated[..interp_count],
                0,
                pos_y,
                0,
            );

            // Use aquifer to determine block state (matches vanilla's getInterpolatedState)
            let opaque = match aquifer.compute_substance(noises, block_x, pos_y, block_z, density) {
                crate::chunk::aquifer::AquiferResult::Solid => true,
                crate::chunk::aquifer::AquiferResult::Fluid(_) => !ocean_floor,
                crate::chunk::aquifer::AquiferResult::Air => false,
            };

            if opaque {
                return pos_y + 1;
            }
        }
    }
    min_y
}

/// Evaluate terrain density at a single block position using cell-based
/// interpolation matching vanilla's `NoiseChunk`: inner functions at 8 cell
/// corners, trilinear interpolation per channel, then outer operations.
fn interpolated_density<N: DimensionNoises>(
    cache: &mut N::ColumnCache,
    noises: &N,
    x: i32,
    y: i32,
    z: i32,
    cell_w: i32,
    cell_h: i32,
) -> f64 {
    let cx = x.div_euclid(cell_w);
    let cy = y.div_euclid(cell_h);
    let cz = z.div_euclid(cell_w);
    let fx = f64::from(x.rem_euclid(cell_w)) / f64::from(cell_w);
    let fy = f64::from(y.rem_euclid(cell_h)) / f64::from(cell_h);
    let fz = f64::from(z.rem_euclid(cell_w)) / f64::from(cell_w);

    let x0 = cx * cell_w;
    let x1 = x0 + cell_w;
    let y0 = cy * cell_h;
    let y1 = y0 + cell_h;
    let z0 = cz * cell_w;
    let z1 = z0 + cell_w;

    let interp_count = N::interpolated_count();

    const MAX_INTERP: usize = 16;
    let mut c000 = [0.0f64; MAX_INTERP];
    let mut c100 = [0.0f64; MAX_INTERP];
    let mut c010 = [0.0f64; MAX_INTERP];
    let mut c110 = [0.0f64; MAX_INTERP];
    let mut c001 = [0.0f64; MAX_INTERP];
    let mut c101 = [0.0f64; MAX_INTERP];
    let mut c011 = [0.0f64; MAX_INTERP];
    let mut c111 = [0.0f64; MAX_INTERP];
    let mut interpolated = [0.0f64; MAX_INTERP];

    macro_rules! fill {
        ($out:expr, $ex:expr, $ey:expr, $ez:expr) => {{
            cache.ensure($ex, $ez, noises);
            noises.fill_cell_corner_densities(
                &mut *cache,
                $ex,
                $ey,
                $ez,
                &mut $out[..interp_count],
            );
        }};
    }

    fill!(c000, x0, y0, z0);
    fill!(c100, x1, y0, z0);
    fill!(c010, x0, y1, z0);
    fill!(c110, x1, y1, z0);
    fill!(c001, x0, y0, z1);
    fill!(c101, x1, y0, z1);
    fill!(c011, x0, y1, z1);
    fill!(c111, x1, y1, z1);

    use steel_utils::math::noise_math::lerp;
    for ch in 0..interp_count {
        let d00 = lerp(fy, c000[ch], c010[ch]);
        let d10 = lerp(fy, c100[ch], c110[ch]);
        let d01 = lerp(fy, c001[ch], c011[ch]);
        let d11 = lerp(fy, c101[ch], c111[ch]);
        let d0 = lerp(fx, d00, d10);
        let d1 = lerp(fx, d01, d11);
        interpolated[ch] = lerp(fz, d0, d1);
    }

    noises.combine_interpolated(&mut *cache, &interpolated[..interp_count], 0, y, 0)
}

impl<N: DimensionNoises> VanillaGenerator<N> {
    /// Returns the first Y from the top where terrain is solid.
    ///
    /// Approximates vanilla's `getBaseHeight` / `getFirstOccupiedHeight` by
    /// scanning downward from the preliminary surface estimate using direct
    /// density evaluation. Starts 16 blocks above the estimate and scans
    /// down to `min_y`.
    fn get_base_height(&self, x: i32, z: i32, cache: &mut N::ColumnCache) -> i32 {
        let estimate = preliminary_surface_level::<N>(&self.noises, cache, x, z);
        let start_y = (estimate + 16).min(N::Settings::MIN_Y + N::Settings::HEIGHT - 1);
        let min_y = N::Settings::MIN_Y;

        for y in (min_y..=start_y).rev() {
            cache.ensure(x, z, &self.noises);
            let density = self.noises.router_final_density(cache, x, y, z);
            if density > 0.0 {
                return y + 1;
            }
        }

        min_y
    }
}

impl<N: DimensionNoises> ChunkGenerator for VanillaGenerator<N> {
    fn create_structures(&self, chunk: &ChunkAccess) {
        let pos = chunk.pos();
        let chunk_x = pos.0.x;
        let chunk_z = pos.0.y;

        let mut sampler = self.biome_source.chunk_sampler();
        let chunk_min_x = chunk_x * 16;
        let chunk_min_z = chunk_z * 16;
        let center_block_x = chunk_min_x + 8;
        let center_block_z = chunk_min_z + 8;

        let mut height_cache = N::ColumnCache::default();
        let sea_level = N::Settings::SEA_LEVEL;

        let cell_w = N::Settings::CELL_WIDTH;
        let cell_h = N::Settings::CELL_HEIGHT;

        // Create an aquifer for this chunk to match vanilla's iterateNoiseColumn,
        // which creates a NoiseChunk (containing an aquifer) per height query.
        // We reuse one aquifer for all queries in this chunk — the grid extends
        // beyond the 16-block chunk boundary due to sample offsets, covering
        // nearby positions needed by structure placement.
        let mut aquifer_cache = N::ColumnCache::default();
        aquifer_cache.init_grid(chunk_min_x, chunk_min_z, &self.noises);
        let mut aquifer = Aquifer::<N>::new(
            chunk_min_x,
            chunk_min_z,
            N::Settings::MIN_Y,
            N::Settings::HEIGHT,
            &self.splitter,
            &self.noises,
            aquifer_cache,
        );

        // Aquifer-aware height scan matching vanilla's iterateNoiseColumn.
        // Uses trilinear-interpolated density + aquifer to determine block state,
        // then checks against the heightmap predicate.
        // WORLD_SURFACE_WG (ocean_floor=false): opaque = Solid or Fluid
        // OCEAN_FLOOR_WG (ocean_floor=true): opaque = Solid only
        let base_height = |cache: &mut N::ColumnCache,
                           noises: &N,
                           aquifer: &mut Aquifer<N>,
                           x: i32,
                           z: i32,
                           ocean_floor: bool|
         -> i32 {
            let estimate = preliminary_surface_level::<N>(noises, cache, x, z);
            let start_y = (estimate + 16).min(N::Settings::MIN_Y + N::Settings::HEIGHT - 1);
            let min_y = N::Settings::MIN_Y;
            for y in (min_y..=start_y).rev() {
                let density = interpolated_density::<N>(cache, noises, x, y, z, cell_w, cell_h);
                let opaque = match aquifer.compute_substance(noises, x, y, z, density) {
                    AquiferResult::Solid => true,
                    AquiferResult::Fluid(_) => !ocean_floor,
                    AquiferResult::Air => false,
                };
                if opaque {
                    return y + 1;
                }
            }
            min_y
        };

        // onTopOfChunkCenter uses getFirstOccupiedHeight = getBaseHeight - 1
        let surface_y = base_height(
            &mut height_cache,
            &self.noises,
            &mut aquifer,
            center_block_x,
            center_block_z,
            false,
        ) - 1;

        // Collect selected structures first (while can_generate borrows height_cache),
        // then run jigsaw assembly afterward (which also needs height_cache).
        //
        // For jigsaw sets: store the full entry list so post-processing can do
        // weighted selection + assembly + biome check with retry (matching vanilla).
        // For non-jigsaw sets: store just the selected entry.
        enum SelectedSet {
            /// Non-jigsaw: already selected by `can_generate` + weighted pick.
            Single {
                structure: Identifier,
                structure_type: String,
            },
            /// Jigsaw: needs assembly + biome check with retry in post-processing.
            Jigsaw(Vec<StructureSelectionEntry>),
        }
        let mut selected_sets: Vec<SelectedSet> = Vec::new();

        // Checks if findGenerationPoint would succeed and if the biome at the
        // generation point matches. Returns false if the structure type rejects
        // this location or the biome doesn't match.
        let mut can_generate = |entry: &StructureSelectionEntry| -> bool {
            let (biome_x, biome_y, biome_z) = match entry.structure_type.as_str() {
                // Mineshaft: generate pieces to compute bounding box, then
                // moveBelowSeaLevel to get the correct biome check Y
                "minecraft:mineshaft" => {
                    let mtype = if &*entry.structure.path == "mineshaft_mesa" {
                        MineshaftType::Mesa
                    } else {
                        MineshaftType::Normal
                    };
                    // Clone the chunk's random state: vanilla seeds with
                    // setLargeFeatureSeed(seed, chunkX, chunkZ)
                    let mut ms_rng = LegacyRandom::from_seed(0);
                    ms_rng.set_large_feature_seed(self.seed, chunk_x, chunk_z);
                    let mut get_height = |x: i32, z: i32| -> i32 {
                        // Match vanilla's getBaseHeight which uses iterateNoiseColumn
                        // (cell-based interpolation + aquifer)
                        let cw = N::Settings::CELL_WIDTH;
                        let cell_x = x.div_euclid(cw) * cw;
                        let cell_z = z.div_euclid(cw) * cw;
                        let aq_chunk_x = (cell_x >> 4) * 16;
                        let aq_chunk_z = (cell_z >> 4) * 16;
                        let aq_cache = N::ColumnCache::default();
                        let mut fresh_aq = Aquifer::<N>::new(
                            aq_chunk_x,
                            aq_chunk_z,
                            N::Settings::MIN_Y,
                            N::Settings::HEIGHT,
                            &self.splitter,
                            &self.noises,
                            aq_cache,
                        );
                        let mut fresh_cache = N::ColumnCache::default();
                        fresh_cache.init_grid(aq_chunk_x, aq_chunk_z, &self.noises);
                        iterate_noise_column_with_aquifer::<N>(
                            &mut fresh_cache,
                            &self.noises,
                            &mut fresh_aq,
                            x,
                            z,
                            false,
                        )
                    };
                    let result = mineshaft::find_generation_point(
                        &mut ms_rng,
                        chunk_x,
                        chunk_z,
                        mtype,
                        sea_level,
                        N::Settings::MIN_Y,
                        &mut get_height,
                    );
                    result.biome_check_pos
                }

                // SinglePieceStructure (desert_pyramid, jungle_temple):
                // Reject if lowest corner height < sea level.
                // Vanilla uses getLowestY which calls getBaseHeight (interpolated).
                "minecraft:desert_pyramid" | "minecraft:jungle_temple" => {
                    let (width, depth) = match entry.structure_type.as_str() {
                        "minecraft:desert_pyramid" => (21, 21),
                        _ => (12, 15),
                    };
                    // Vanilla uses getFirstOccupiedHeight(WORLD_SURFACE_WG) = getBaseHeight - 1.
                    // interp_base_height returns getBaseHeight (posY + 1), so subtract 1.
                    let h0 = base_height(
                        &mut height_cache,
                        &self.noises,
                        &mut aquifer,
                        chunk_min_x,
                        chunk_min_z,
                        false,
                    ) - 1;
                    let h1 = base_height(
                        &mut height_cache,
                        &self.noises,
                        &mut aquifer,
                        chunk_min_x,
                        chunk_min_z + depth,
                        false,
                    ) - 1;
                    let h2 = base_height(
                        &mut height_cache,
                        &self.noises,
                        &mut aquifer,
                        chunk_min_x + width,
                        chunk_min_z,
                        false,
                    ) - 1;
                    let h3 = base_height(
                        &mut height_cache,
                        &self.noises,
                        &mut aquifer,
                        chunk_min_x + width,
                        chunk_min_z + depth,
                        false,
                    ) - 1;
                    let lowest = h0.min(h1).min(h2).min(h3);
                    if lowest < sea_level {
                        return false;
                    }
                    (center_block_x, surface_y, center_block_z)
                }

                // OceanMonument: check all biomes in 29-block radius are deep ocean
                // OceanMonument: reject if any biome in 29-block radius is not
                // in #minecraft:required_ocean_monument_surrounding (all ocean + river)
                "minecraft:ocean_monument" => {
                    use steel_utils::Identifier;
                    const OCEAN_MONUMENT_SURROUNDING: &[&str] = &[
                        "deep_frozen_ocean",
                        "deep_cold_ocean",
                        "deep_ocean",
                        "deep_lukewarm_ocean",
                        "frozen_ocean",
                        "cold_ocean",
                        "ocean",
                        "lukewarm_ocean",
                        "warm_ocean",
                        "river",
                        "frozen_river",
                    ];

                    let check_x = chunk_min_x + 9;
                    let check_z = chunk_min_z + 9;
                    let check_y = sea_level;
                    let radius = 29;

                    let q_x0 = (check_x - radius) >> 2;
                    let q_x1 = (check_x + radius) >> 2;
                    let q_z0 = (check_z - radius) >> 2;
                    let q_z1 = (check_z + radius) >> 2;
                    let q_y0 = (check_y - radius) >> 2;
                    let q_y1 = (check_y + radius) >> 2;

                    for qz in q_z0..=q_z1 {
                        for qx in q_x0..=q_x1 {
                            for qy in q_y0..=q_y1 {
                                let biome = sampler.sample(qx, qy, qz);
                                let is_surrounding = OCEAN_MONUMENT_SURROUNDING
                                    .iter()
                                    .any(|&b| biome.key == Identifier::vanilla_static(b));
                                if !is_surrounding {
                                    return false;
                                }
                            }
                        }
                    }
                    (center_block_x, surface_y, center_block_z)
                }

                // RuinedPortal: run vanilla's findGenerationPoint RNG to get
                // the correct biome check Y (surface vs underground).
                // Vanilla creates a fresh NoiseChunk (and aquifer) per column
                // query, so we must do the same for corners that may span
                // different chunks.
                "minecraft:ruined_portal" => {
                    use crate::world::structure::ruined_portal::{TerrainQuery, TerrainResult};
                    let mut rp_rng = LegacyRandom::from_seed(0);
                    rp_rng.set_large_feature_seed(self.seed, chunk_x, chunk_z);
                    let mut terrain = |q: TerrainQuery| -> TerrainResult {
                        let (qx, qz) = match q {
                            TerrainQuery::SurfaceHeight(x, z) => (x, z),
                            TerrainQuery::IsOpaque(x, _, z) => (x, z),
                        };
                        let cw = N::Settings::CELL_WIDTH;
                        let cell_x = qx.div_euclid(cw) * cw;
                        let cell_z = qz.div_euclid(cw) * cw;
                        let aq_chunk_x = (cell_x >> 4) * 16;
                        let aq_chunk_z = (cell_z >> 4) * 16;
                        let aq_cache = N::ColumnCache::default();
                        let mut fresh_aq = Aquifer::<N>::new(
                            aq_chunk_x,
                            aq_chunk_z,
                            N::Settings::MIN_Y,
                            N::Settings::HEIGHT,
                            &self.splitter,
                            &self.noises,
                            aq_cache,
                        );
                        let mut fresh_cache = N::ColumnCache::default();
                        fresh_cache.init_grid(aq_chunk_x, aq_chunk_z, &self.noises);
                        match q {
                            TerrainQuery::SurfaceHeight(x, z) => {
                                TerrainResult::Height(iterate_noise_column_with_aquifer::<N>(
                                    &mut fresh_cache,
                                    &self.noises,
                                    &mut fresh_aq,
                                    x,
                                    z,
                                    false,
                                ))
                            }
                            TerrainQuery::IsOpaque(x, y, z) => {
                                let density = interpolated_density::<N>(
                                    &mut fresh_cache,
                                    &self.noises,
                                    x,
                                    y,
                                    z,
                                    cell_w,
                                    cell_h,
                                );
                                let opaque = match fresh_aq.compute_substance(
                                    &self.noises,
                                    x,
                                    y,
                                    z,
                                    density,
                                ) {
                                    AquiferResult::Solid | AquiferResult::Fluid(_) => true,
                                    AquiferResult::Air => false,
                                };
                                TerrainResult::Opaque(opaque)
                            }
                        }
                    };
                    let result = ruined_portal::find_generation_point(
                        &mut rp_rng,
                        chunk_x,
                        chunk_z,
                        &entry.structure.path,
                        N::Settings::MIN_Y,
                        &mut terrain,
                    );
                    result.biome_check_pos
                }

                // onTopOfChunkCenter with WORLD_SURFACE_WG heightmap
                "minecraft:shipwreck" | "minecraft:swamp_hut" | "minecraft:igloo" => {
                    (center_block_x, surface_y, center_block_z)
                }

                // onTopOfChunkCenter with OCEAN_FLOOR_WG heightmap
                "minecraft:ocean_ruin" | "minecraft:buried_treasure" => {
                    let ocean_floor_y = base_height(
                        &mut height_cache,
                        &self.noises,
                        &mut aquifer,
                        center_block_x,
                        center_block_z,
                        true,
                    );
                    (center_block_x, ocean_floor_y, center_block_z)
                }

                // Woodland mansion / End city: offset position at chunkPos.getBlockX(7),
                // getBlockZ(7) with getLowestY in 5x5 box. Reject if Y < 60.
                // Uses getFirstOccupiedHeight(WORLD_SURFACE_WG) = getBaseHeight - 1.
                "minecraft:woodland_mansion" | "minecraft:end_city" => {
                    let bx = chunk_min_x + 7;
                    let bz = chunk_min_z + 7;
                    let h0 =
                        base_height(&mut height_cache, &self.noises, &mut aquifer, bx, bz, false)
                            - 1;
                    let h1 = base_height(
                        &mut height_cache,
                        &self.noises,
                        &mut aquifer,
                        bx,
                        bz + 5,
                        false,
                    ) - 1;
                    let h2 = base_height(
                        &mut height_cache,
                        &self.noises,
                        &mut aquifer,
                        bx + 5,
                        bz,
                        false,
                    ) - 1;
                    let h3 = base_height(
                        &mut height_cache,
                        &self.noises,
                        &mut aquifer,
                        bx + 5,
                        bz + 5,
                        false,
                    ) - 1;
                    let lowest = h0.min(h1).min(h2).min(h3);
                    if lowest < 60 {
                        return false;
                    }
                    (bx, lowest, bz)
                }

                // Nether fortress: fixed Y=64 at chunk origin
                "minecraft:fortress" => (chunk_min_x, 64, chunk_min_z),

                // Nether fossil: complex Y with RNG, nether only
                "minecraft:nether_fossil" => {
                    // TODO: Implement full nether fossil height logic
                    (center_block_x, surface_y, center_block_z)
                }

                // Jigsaw structures (villages, trail_ruins, trial_chambers, etc.):
                // biome check at center, Y from biome_check_y or surface.
                "minecraft:jigsaw" => {
                    let y = entry.biome_check_y.unwrap_or(surface_y);
                    (center_block_x, y, center_block_z)
                }

                // Stronghold: uses ConcentricRings placement with biome snapping.
                // Biome check at center, surface Y.
                "minecraft:stronghold" => (center_block_x, surface_y, center_block_z),

                other => {
                    tracing::warn!(
                        "Unknown structure type {other:?} for {}, using center position",
                        entry.structure
                    );
                    (center_block_x, surface_y, center_block_z)
                }
            };

            let biome = sampler.sample(biome_x >> 2, biome_y >> 2, biome_z >> 2);
            entry.allowed_biomes.contains(&biome.key)
        };

        for (set_key, set) in &self.structure_sets {
            let is_jigsaw_set = set
                .structures
                .iter()
                .any(|e| e.structure_type == "minecraft:jigsaw");

            // For jigsaw sets: skip biome pre-check (biome is checked post-assembly).
            // For non-jigsaw sets: require at least one entry to pass biome check.
            if !is_jigsaw_set && !set.structures.iter().any(&mut can_generate) {
                continue;
            }

            // Skip if any structure in this set already has a valid start
            {
                let starts = chunk.structure_starts();
                let already_has = set.structures.iter().any(|entry| {
                    starts
                        .get(&entry.structure)
                        .is_some_and(|s| !s.pieces.is_empty())
                });
                if already_has {
                    continue;
                }
            }

            // Look up pre-computed ring positions for this set (if ConcentricRings)
            let rings = self
                .ring_positions
                .iter()
                .find(|(k, _)| k == set_key)
                .map(|(_, pos)| pos.as_slice());

            if !set
                .placement
                .is_structure_chunk(self.seed, chunk_x, chunk_z, rings)
            {
                continue;
            }

            // Exclusion zone: skip if another set has a structure chunk nearby
            if set.placement.is_excluded(
                self.seed,
                chunk_x,
                chunk_z,
                &self.structure_sets,
                &self.ring_positions,
            ) {
                continue;
            }

            if is_jigsaw_set {
                // Jigsaw: defer weighted selection + assembly + biome check to post-processing.
                // This matches vanilla's flow where tryGenerateStructure runs the full
                // assembly before checking the biome at the stub position.
                selected_sets.push(SelectedSet::Jigsaw(set.structures.clone()));
            } else {
                // Non-jigsaw: weighted selection with biome check via can_generate.
                let selected = if set.structures.len() == 1 {
                    let entry = &set.structures[0];
                    if can_generate(entry) {
                        Some(entry)
                    } else {
                        None
                    }
                } else {
                    let mut rng = LegacyRandom::from_seed(0);
                    rng.set_large_feature_seed(self.seed, chunk_x, chunk_z);

                    let mut remaining: Vec<&StructureSelectionEntry> =
                        set.structures.iter().collect();
                    let mut total_weight: i32 = remaining.iter().map(|e| e.weight).sum();
                    let mut result = None;

                    while !remaining.is_empty() {
                        let mut choice = rng.next_i32_bounded(total_weight);
                        let mut selected_idx = 0;
                        for (i, entry) in remaining.iter().enumerate() {
                            choice -= entry.weight;
                            if choice < 0 {
                                selected_idx = i;
                                break;
                            }
                        }

                        let candidate = remaining[selected_idx];
                        if can_generate(candidate) {
                            result = Some(candidate);
                            break;
                        }

                        total_weight -= candidate.weight;
                        remaining.remove(selected_idx);
                    }

                    result
                };

                if let Some(selected) = selected {
                    selected_sets.push(SelectedSet::Single {
                        structure: selected.structure.clone(),
                        structure_type: selected.structure_type.clone(),
                    });
                }
            }
        }

        // can_generate is dropped here, releasing the height_cache borrow.
        // Now process selected sets — run jigsaw assembly with retry for jigsaw structures.
        for selected in selected_sets {
            match selected {
                SelectedSet::Single {
                    structure: structure_id,
                    structure_type,
                } => {
                    // Non-jigsaw: generate pieces based on structure type.
                    let pieces = match structure_type.as_str() {
                        "minecraft:mineshaft" => {
                            let mtype = if &*structure_id.path == "mineshaft_mesa" {
                                MineshaftType::Mesa
                            } else {
                                MineshaftType::Normal
                            };
                            let mut ms_rng = LegacyRandom::from_seed(0);
                            ms_rng.set_large_feature_seed(self.seed, chunk_x, chunk_z);
                            let mut get_height = |x: i32, z: i32| -> i32 {
                                let cw = N::Settings::CELL_WIDTH;
                                let cell_x = x.div_euclid(cw) * cw;
                                let cell_z = z.div_euclid(cw) * cw;
                                let aq_chunk_x = (cell_x >> 4) * 16;
                                let aq_chunk_z = (cell_z >> 4) * 16;
                                let aq_cache = N::ColumnCache::default();
                                let mut fresh_aq = Aquifer::<N>::new(
                                    aq_chunk_x,
                                    aq_chunk_z,
                                    N::Settings::MIN_Y,
                                    N::Settings::HEIGHT,
                                    &self.splitter,
                                    &self.noises,
                                    aq_cache,
                                );
                                let mut fresh_cache = N::ColumnCache::default();
                                fresh_cache.init_grid(aq_chunk_x, aq_chunk_z, &self.noises);
                                iterate_noise_column_with_aquifer::<N>(
                                    &mut fresh_cache,
                                    &self.noises,
                                    &mut fresh_aq,
                                    x,
                                    z,
                                    false,
                                )
                            };
                            let result = mineshaft::find_generation_point(
                                &mut ms_rng,
                                chunk_x,
                                chunk_z,
                                mtype,
                                sea_level,
                                N::Settings::MIN_Y,
                                &mut get_height,
                            );
                            result
                                .piece_bbs
                                .into_iter()
                                .map(|bb| StructurePiece {
                                    piece_type: Identifier::new_static("minecraft", "mineshaft"),
                                    bounding_box: bb,
                                    gen_depth: 0,
                                    orientation: None,
                                    nbt_data: Vec::new(),
                                    ground_level_delta: 0,
                                    junctions: Vec::new(),
                                })
                                .collect()
                        }
                        // SinglePieceStructure pattern (desert_pyramid, jungle_temple, swamp_hut):
                        // Piece BB from makeBoundingBox(west, 64, north, randomDir, width, height, depth).
                        // Direction axis Z (N/S): (x..x+w-1, y..y+h-1, z..z+d-1)
                        // Direction axis X (E/W): (x..x+d-1, y..y+h-1, z..z+w-1)
                        "minecraft:desert_pyramid"
                        | "minecraft:jungle_temple"
                        | "minecraft:swamp_hut" => {
                            let (w, h, d, piece_type_name) = match structure_type.as_str() {
                                "minecraft:desert_pyramid" => (21, 15, 21, "desert_pyramid"),
                                "minecraft:jungle_temple" => (12, 10, 15, "jungle_pyramid"),
                                _ => (7, 7, 9, "swamp_hut"),
                            };
                            // Fresh RNG matching vanilla's GenerationContext
                            let mut piece_rng = LegacyRandom::from_seed(0);
                            piece_rng.set_large_feature_seed(self.seed, chunk_x, chunk_z);
                            // getRandomHorizontalDirection: HORIZONTAL faces = [N, E, S, W]
                            let dir_idx = piece_rng.next_i32_bounded(4);
                            let z_axis = matches!(dir_idx, 0 | 2); // N=0, E=1, S=2, W=3
                            let (bw, bd) = if z_axis { (w, d) } else { (d, w) };
                            vec![StructurePiece {
                                piece_type: Identifier::new_static("minecraft", piece_type_name),
                                bounding_box: BoundingBox::new(
                                    chunk_min_x,
                                    64,
                                    chunk_min_z,
                                    chunk_min_x + bw - 1,
                                    64 + h - 1,
                                    chunk_min_z + bd - 1,
                                ),
                                gen_depth: 0,
                                orientation: None,
                                nbt_data: Vec::new(),
                                ground_level_delta: 0,
                                junctions: Vec::new(),
                            }]
                        }
                        "minecraft:buried_treasure" => {
                            vec![StructurePiece {
                                piece_type: Identifier::new_static("minecraft", "buried_treasure"),
                                bounding_box: BoundingBox::new(
                                    chunk_min_x + 9,
                                    90,
                                    chunk_min_z + 9,
                                    chunk_min_x + 9,
                                    90,
                                    chunk_min_z + 9,
                                ),
                                gen_depth: 0,
                                orientation: None,
                                nbt_data: Vec::new(),
                                ground_level_delta: 0,
                                junctions: Vec::new(),
                            }]
                        }
                        "minecraft:ruined_portal" => {
                            use crate::world::structure::ruined_portal::{
                                TerrainQuery, TerrainResult,
                            };
                            let mut rp_rng = LegacyRandom::from_seed(0);
                            rp_rng.set_large_feature_seed(self.seed, chunk_x, chunk_z);
                            let mut terrain = |q: TerrainQuery| -> TerrainResult {
                                let (qx, qz) = match q {
                                    TerrainQuery::SurfaceHeight(x, z) => (x, z),
                                    TerrainQuery::IsOpaque(x, _, z) => (x, z),
                                };
                                let cw = N::Settings::CELL_WIDTH;
                                let cell_x = qx.div_euclid(cw) * cw;
                                let cell_z = qz.div_euclid(cw) * cw;
                                let aq_chunk_x = (cell_x >> 4) * 16;
                                let aq_chunk_z = (cell_z >> 4) * 16;
                                let aq_cache = N::ColumnCache::default();
                                let mut fresh_aq = Aquifer::<N>::new(
                                    aq_chunk_x,
                                    aq_chunk_z,
                                    N::Settings::MIN_Y,
                                    N::Settings::HEIGHT,
                                    &self.splitter,
                                    &self.noises,
                                    aq_cache,
                                );
                                let mut fresh_cache = N::ColumnCache::default();
                                fresh_cache.init_grid(aq_chunk_x, aq_chunk_z, &self.noises);
                                match q {
                                    TerrainQuery::SurfaceHeight(x, z) => TerrainResult::Height(
                                        iterate_noise_column_with_aquifer::<N>(
                                            &mut fresh_cache,
                                            &self.noises,
                                            &mut fresh_aq,
                                            x,
                                            z,
                                            false,
                                        ),
                                    ),
                                    TerrainQuery::IsOpaque(x, y, z) => {
                                        let density = interpolated_density::<N>(
                                            &mut fresh_cache,
                                            &self.noises,
                                            x,
                                            y,
                                            z,
                                            cell_w,
                                            cell_h,
                                        );
                                        let opaque = match fresh_aq.compute_substance(
                                            &self.noises,
                                            x,
                                            y,
                                            z,
                                            density,
                                        ) {
                                            AquiferResult::Solid | AquiferResult::Fluid(_) => true,
                                            AquiferResult::Air => false,
                                        };
                                        TerrainResult::Opaque(opaque)
                                    }
                                }
                            };
                            let result = ruined_portal::find_generation_point(
                                &mut rp_rng,
                                chunk_x,
                                chunk_z,
                                &structure_id.path,
                                N::Settings::MIN_Y,
                                &mut terrain,
                            );
                            vec![StructurePiece {
                                piece_type: Identifier::new_static("minecraft", "ruined_portal"),
                                bounding_box: result.bounding_box,
                                gen_depth: 0,
                                orientation: None,
                                nbt_data: Vec::new(),
                                ground_level_delta: 0,
                                junctions: Vec::new(),
                            }]
                        }
                        "minecraft:shipwreck" => {
                            // Vanilla: picks random template, random rotation, position at (minBlockX, 90, minBlockZ)
                            // BB from template size + rotation with pivot (4, 0, 15).
                            let mut sw_rng = LegacyRandom::from_seed(0);
                            sw_rng.set_large_feature_seed(self.seed, chunk_x, chunk_z);

                            let is_beached = structure_id.path == "shipwreck_beached";
                            let beached_count = 11i32;
                            let ocean_count = 20i32;
                            let template_count = if is_beached {
                                beached_count
                            } else {
                                ocean_count
                            };

                            static BEACHED: &[&str] = &[
                                "shipwreck/with_mast",
                                "shipwreck/sideways_full",
                                "shipwreck/sideways_fronthalf",
                                "shipwreck/sideways_backhalf",
                                "shipwreck/rightsideup_full",
                                "shipwreck/rightsideup_fronthalf",
                                "shipwreck/rightsideup_backhalf",
                                "shipwreck/with_mast_degraded",
                                "shipwreck/rightsideup_full_degraded",
                                "shipwreck/rightsideup_fronthalf_degraded",
                                "shipwreck/rightsideup_backhalf_degraded",
                            ];
                            static OCEAN: &[&str] = &[
                                "shipwreck/with_mast",
                                "shipwreck/upsidedown_full",
                                "shipwreck/upsidedown_fronthalf",
                                "shipwreck/upsidedown_backhalf",
                                "shipwreck/sideways_full",
                                "shipwreck/sideways_fronthalf",
                                "shipwreck/sideways_backhalf",
                                "shipwreck/rightsideup_full",
                                "shipwreck/rightsideup_fronthalf",
                                "shipwreck/rightsideup_backhalf",
                                "shipwreck/with_mast_degraded",
                                "shipwreck/upsidedown_full_degraded",
                                "shipwreck/upsidedown_fronthalf_degraded",
                                "shipwreck/upsidedown_backhalf_degraded",
                                "shipwreck/sideways_full_degraded",
                                "shipwreck/sideways_fronthalf_degraded",
                                "shipwreck/sideways_backhalf_degraded",
                                "shipwreck/rightsideup_full_degraded",
                                "shipwreck/rightsideup_fronthalf_degraded",
                                "shipwreck/rightsideup_backhalf_degraded",
                            ];

                            // Rotation.getRandom
                            let rotation = Rotation::get_random(&mut sw_rng);
                            // Util.getRandom picks template
                            let templates_arr = if is_beached { BEACHED } else { OCEAN };
                            let template_idx = sw_rng.next_i32_bounded(template_count) as usize;
                            let template_name =
                                Identifier::new("minecraft", templates_arr[template_idx]);

                            if let Some(tmpl) = self.templates.get(&template_name) {
                                let bb = rotation.get_bounding_box_with_pivot(
                                    chunk_min_x,
                                    90,
                                    chunk_min_z,
                                    tmpl.size[0],
                                    tmpl.size[1],
                                    tmpl.size[2],
                                    4,
                                    15, // Shipwreck pivot
                                );
                                vec![StructurePiece {
                                    piece_type: Identifier::new_static("minecraft", "shipwreck"),
                                    bounding_box: bb,
                                    gen_depth: 0,
                                    orientation: None,
                                    nbt_data: Vec::new(),
                                    ground_level_delta: 0,
                                    junctions: Vec::new(),
                                }]
                            } else {
                                vec![]
                            }
                        }
                        "minecraft:ocean_ruin" => {
                            // Full ocean ruin piece generation matching vanilla's OceanRuinPieces.addPieces.
                            let is_warm = structure_id.path.contains("warm");
                            let large_prob: f32 = 0.3;
                            let cluster_prob: f32 = 0.9;

                            let mut or_rng = LegacyRandom::from_seed(0);
                            or_rng.set_large_feature_seed(self.seed, chunk_x, chunk_z);

                            let rotation = Rotation::get_random(&mut or_rng);

                            // isLarge check
                            let is_large = or_rng.next_f32() <= large_prob;

                            // Template arrays
                            static WARM_SMALL: &[&str] = &[
                                "underwater_ruin/warm_1",
                                "underwater_ruin/warm_2",
                                "underwater_ruin/warm_3",
                                "underwater_ruin/warm_4",
                                "underwater_ruin/warm_5",
                                "underwater_ruin/warm_6",
                                "underwater_ruin/warm_7",
                                "underwater_ruin/warm_8",
                            ];
                            static WARM_LARGE: &[&str] = &[
                                "underwater_ruin/big_warm_4",
                                "underwater_ruin/big_warm_5",
                                "underwater_ruin/big_warm_6",
                                "underwater_ruin/big_warm_7",
                            ];
                            static COLD_BRICK: &[&str] = &[
                                "underwater_ruin/brick_1",
                                "underwater_ruin/brick_2",
                                "underwater_ruin/brick_3",
                                "underwater_ruin/brick_4",
                                "underwater_ruin/brick_5",
                                "underwater_ruin/brick_6",
                                "underwater_ruin/brick_7",
                                "underwater_ruin/brick_8",
                            ];
                            static COLD_CRACKED: &[&str] = &[
                                "underwater_ruin/cracked_1",
                                "underwater_ruin/cracked_2",
                                "underwater_ruin/cracked_3",
                                "underwater_ruin/cracked_4",
                                "underwater_ruin/cracked_5",
                                "underwater_ruin/cracked_6",
                                "underwater_ruin/cracked_7",
                                "underwater_ruin/cracked_8",
                            ];
                            static COLD_MOSSY: &[&str] = &[
                                "underwater_ruin/mossy_1",
                                "underwater_ruin/mossy_2",
                                "underwater_ruin/mossy_3",
                                "underwater_ruin/mossy_4",
                                "underwater_ruin/mossy_5",
                                "underwater_ruin/mossy_6",
                                "underwater_ruin/mossy_7",
                                "underwater_ruin/mossy_8",
                            ];
                            static COLD_BIG_BRICK: &[&str] = &[
                                "underwater_ruin/big_brick_1",
                                "underwater_ruin/big_brick_2",
                                "underwater_ruin/big_brick_3",
                                "underwater_ruin/big_brick_8",
                            ];
                            static COLD_BIG_CRACKED: &[&str] = &[
                                "underwater_ruin/big_cracked_1",
                                "underwater_ruin/big_cracked_2",
                                "underwater_ruin/big_cracked_3",
                                "underwater_ruin/big_cracked_8",
                            ];
                            static COLD_BIG_MOSSY: &[&str] = &[
                                "underwater_ruin/big_mossy_1",
                                "underwater_ruin/big_mossy_2",
                                "underwater_ruin/big_mossy_3",
                                "underwater_ruin/big_mossy_8",
                            ];

                            let mut pieces = Vec::new();
                            let pos = (chunk_min_x, 90, chunk_min_z);

                            // Add base piece(s)
                            let add_piece_bb = |templates: &rustc_hash::FxHashMap<
                                Identifier,
                                steel_registry::template_pool::TemplateData,
                            >,
                                                name: &str,
                                                px: i32,
                                                pz: i32,
                                                rot: Rotation|
                             -> Option<BoundingBox> {
                                let key = Identifier::new("minecraft", name.to_string());
                                templates.get(&key).map(|t| {
                                    rot.get_bounding_box(
                                        px, 90, pz, t.size[0], t.size[1], t.size[2],
                                    )
                                })
                            };

                            if is_warm {
                                let arr = if is_large { WARM_LARGE } else { WARM_SMALL };
                                let idx = or_rng.next_i32_bounded(arr.len() as i32) as usize;
                                if let Some(bb) =
                                    add_piece_bb(&self.templates, arr[idx], pos.0, pos.2, rotation)
                                {
                                    pieces.push(bb);
                                }
                            } else {
                                let bricks = if is_large { COLD_BIG_BRICK } else { COLD_BRICK };
                                let cracked = if is_large {
                                    COLD_BIG_CRACKED
                                } else {
                                    COLD_CRACKED
                                };
                                let mossy = if is_large { COLD_BIG_MOSSY } else { COLD_MOSSY };
                                let idx = or_rng.next_i32_bounded(bricks.len() as i32) as usize;
                                if let Some(bb) = add_piece_bb(
                                    &self.templates,
                                    bricks[idx],
                                    pos.0,
                                    pos.2,
                                    rotation,
                                ) {
                                    pieces.push(bb);
                                }
                                if let Some(bb) = add_piece_bb(
                                    &self.templates,
                                    cracked[idx],
                                    pos.0,
                                    pos.2,
                                    rotation,
                                ) {
                                    pieces.push(bb);
                                }
                                if let Some(bb) = add_piece_bb(
                                    &self.templates,
                                    mossy[idx],
                                    pos.0,
                                    pos.2,
                                    rotation,
                                ) {
                                    pieces.push(bb);
                                }
                            }

                            // Cluster ruins (if large and cluster check passes)
                            if is_large && or_rng.next_f32() <= cluster_prob {
                                // Compute parent corner for collision check
                                let (pc_x, _, pc_z) = rotation.transform_pos(15, 0, 15, 0, 0);
                                let parent_corner_x = pos.0 + pc_x;
                                let parent_corner_z = pos.2 + pc_z;
                                let parent_bb = BoundingBox::new(
                                    pos.0.min(parent_corner_x),
                                    0,
                                    pos.2.min(parent_corner_z),
                                    pos.0.max(parent_corner_x),
                                    255,
                                    pos.2.max(parent_corner_z),
                                );
                                let bottom_left_x = pos.0.min(parent_corner_x);
                                let bottom_left_z = pos.2.min(parent_corner_z);

                                // Generate 8 candidate positions
                                let mut candidates = Vec::with_capacity(8);
                                candidates.push((
                                    bottom_left_x - 16 + or_rng.next_i32_between(1, 8),
                                    bottom_left_z + 16 + or_rng.next_i32_between(1, 7),
                                ));
                                candidates.push((
                                    bottom_left_x - 16 + or_rng.next_i32_between(1, 8),
                                    bottom_left_z + or_rng.next_i32_between(1, 7),
                                ));
                                candidates.push((
                                    bottom_left_x - 16 + or_rng.next_i32_between(1, 8),
                                    bottom_left_z - 16 + or_rng.next_i32_between(4, 8),
                                ));
                                candidates.push((
                                    bottom_left_x + or_rng.next_i32_between(1, 7),
                                    bottom_left_z + 16 + or_rng.next_i32_between(1, 7),
                                ));
                                candidates.push((
                                    bottom_left_x + or_rng.next_i32_between(1, 7),
                                    bottom_left_z - 16 + or_rng.next_i32_between(4, 6),
                                ));
                                candidates.push((
                                    bottom_left_x + 16 + or_rng.next_i32_between(1, 7),
                                    bottom_left_z + 16 + or_rng.next_i32_between(3, 8),
                                ));
                                candidates.push((
                                    bottom_left_x + 16 + or_rng.next_i32_between(1, 7),
                                    bottom_left_z + or_rng.next_i32_between(1, 7),
                                ));
                                candidates.push((
                                    bottom_left_x + 16 + or_rng.next_i32_between(1, 7),
                                    bottom_left_z - 16 + or_rng.next_i32_between(4, 8),
                                ));

                                let ruins_count = or_rng.next_i32_between(4, 8);
                                for _ in 0..ruins_count {
                                    if candidates.is_empty() {
                                        break;
                                    }
                                    let idx =
                                        or_rng.next_i32_bounded(candidates.len() as i32) as usize;
                                    let (cx, cz) = candidates.remove(idx);
                                    let cluster_rot = Rotation::get_random(&mut or_rng);
                                    // Check collision with parent
                                    let (nc_x, _, nc_z) = cluster_rot.transform_pos(5, 0, 6, 0, 0);
                                    let cluster_bb = BoundingBox::new(
                                        cx.min(cx + nc_x),
                                        0,
                                        cz.min(cz + nc_z),
                                        cx.max(cx + nc_x),
                                        255,
                                        cz.max(cz + nc_z),
                                    );
                                    if !cluster_bb.intersects(&parent_bb) {
                                        // Pick small template for cluster piece
                                        let cluster_arr =
                                            if is_warm { WARM_SMALL } else { COLD_BRICK };
                                        let tidx = or_rng.next_i32_bounded(cluster_arr.len() as i32)
                                            as usize;
                                        if let Some(bb) = add_piece_bb(
                                            &self.templates,
                                            cluster_arr[tidx],
                                            cx,
                                            cz,
                                            cluster_rot,
                                        ) {
                                            pieces.push(bb);
                                        }
                                    }
                                }
                            }

                            pieces
                                .into_iter()
                                .map(|bb| StructurePiece {
                                    piece_type: Identifier::new_static("minecraft", "ocean_ruin"),
                                    bounding_box: bb,
                                    gen_depth: 0,
                                    orientation: None,
                                    nbt_data: Vec::new(),
                                    ground_level_delta: 0,
                                    junctions: Vec::new(),
                                })
                                .collect()
                        }
                        "minecraft:stronghold" => {
                            use crate::world::structure::stronghold;
                            let piece_bbs =
                                stronghold::generate_pieces(self.seed, chunk_x, chunk_z);
                            piece_bbs
                                .into_iter()
                                .map(|(bb, piece_id)| StructurePiece {
                                    piece_type: Identifier::new_static("minecraft", piece_id),
                                    bounding_box: bb,
                                    gen_depth: 0,
                                    orientation: None,
                                    nbt_data: Vec::new(),
                                    ground_level_delta: 0,
                                    junctions: Vec::new(),
                                })
                                .collect()
                        }
                        "minecraft:igloo" => {
                            // Vanilla: IglooStructure → IglooPieces.addPieces
                            // Uses templates with per-template pivots and offsets.
                            // 50% chance for basement with random depth.
                            let mut ig_rng = LegacyRandom::from_seed(0);
                            ig_rng.set_large_feature_seed(self.seed, chunk_x, chunk_z);

                            let rotation = Rotation::get_random(&mut ig_rng);

                            // Template sizes (from extracted NBT)
                            const TOP_SIZE: [i32; 3] = [7, 5, 8];
                            const MID_SIZE: [i32; 3] = [3, 3, 3];
                            const BOT_SIZE: [i32; 3] = [7, 6, 9];
                            // Rotation pivots
                            const TOP_PIVOT: (i32, i32) = (3, 5);
                            const MID_PIVOT: (i32, i32) = (1, 1);
                            const BOT_PIVOT: (i32, i32) = (3, 7);
                            // Position offsets from start (chunkMinX, 90, chunkMinZ)
                            const TOP_OFF: (i32, i32, i32) = (0, 0, 0);
                            const MID_OFF: (i32, i32, i32) = (2, -3, 4);
                            const BOT_OFF: (i32, i32, i32) = (0, -3, -2);

                            let start_x = chunk_min_x;
                            let start_z = chunk_min_z;
                            const GEN_Y: i32 = 90;

                            let make_piece =
                                |off: (i32, i32, i32),
                                 depth: i32,
                                 size: [i32; 3],
                                 pivot: (i32, i32)| {
                                    let pos_x = start_x + off.0;
                                    let pos_y = GEN_Y + off.1 - depth;
                                    let pos_z = start_z + off.2;
                                    rotation.get_bounding_box_with_pivot(
                                        pos_x, pos_y, pos_z, size[0], size[1], size[2],
                                        pivot.0, pivot.1,
                                    )
                                };

                            let mut pieces = Vec::new();

                            // 50% chance for basement
                            if ig_rng.next_f64() < 0.5_f64 {
                                let depth = ig_rng.next_i32_bounded(8) + 4; // 4..11
                                // Laboratory at the bottom
                                pieces.push(StructurePiece {
                                    piece_type: Identifier::new_static("minecraft", "igloo"),
                                    bounding_box: make_piece(
                                        BOT_OFF, depth * 3, BOT_SIZE, BOT_PIVOT,
                                    ),
                                    gen_depth: 0,
                                    orientation: None,
                                    nbt_data: Vec::new(),
                                    ground_level_delta: 0,
                                    junctions: Vec::new(),
                                });
                                // Ladder segments
                                for i in 0..depth - 1 {
                                    pieces.push(StructurePiece {
                                        piece_type: Identifier::new_static(
                                            "minecraft", "igloo",
                                        ),
                                        bounding_box: make_piece(
                                            MID_OFF, i * 3, MID_SIZE, MID_PIVOT,
                                        ),
                                        gen_depth: 0,
                                        orientation: None,
                                        nbt_data: Vec::new(),
                                        ground_level_delta: 0,
                                        junctions: Vec::new(),
                                    });
                                }
                            }

                            // Top piece (always)
                            pieces.push(StructurePiece {
                                piece_type: Identifier::new_static("minecraft", "igloo"),
                                bounding_box: make_piece(
                                    TOP_OFF, 0, TOP_SIZE, TOP_PIVOT,
                                ),
                                gen_depth: 0,
                                orientation: None,
                                nbt_data: Vec::new(),
                                ground_level_delta: 0,
                                junctions: Vec::new(),
                            });

                            pieces
                        }
                        // TODO: ocean_monument
                        _ => vec![],
                    };

                    let start = StructureStart {
                        structure: structure_id.clone(),
                        chunk_pos: steel_utils::ChunkPos::new(chunk_x, chunk_z),
                        references: 0,
                        pieces,
                        bb_inflate: terrain_adapt_inflate(&structure_id),
                    };
                    chunk.structure_starts_mut().insert(structure_id, start);
                }
                SelectedSet::Jigsaw(entries) => {
                    // Jigsaw: weighted selection with assembly + biome check retry.
                    // Matches vanilla's flow in ChunkGenerator.createStructures:
                    // pick entry → tryGenerateStructure (assembly + biome) → retry if fails.
                    let mut rng = LegacyRandom::from_seed(0);
                    rng.set_large_feature_seed(self.seed, chunk_x, chunk_z);

                    let mut remaining: Vec<&StructureSelectionEntry> = entries.iter().collect();
                    let mut total_weight: i32 = remaining.iter().map(|e| e.weight).sum();

                    while !remaining.is_empty() {
                        let mut choice = rng.next_i32_bounded(total_weight);
                        let mut selected_idx = 0;
                        for (i, entry) in remaining.iter().enumerate() {
                            choice -= entry.weight;
                            if choice < 0 {
                                selected_idx = i;
                                break;
                            }
                        }

                        let candidate = remaining[selected_idx];
                        let Some(ref jigsaw_config) = candidate.jigsaw_config else {
                            remaining.remove(selected_idx);
                            total_weight -= candidate.weight;
                            continue;
                        };

                        // Run assembly for this candidate
                        let mut alias_rng = LegacyRandom::from_seed(0);
                        alias_rng.set_large_feature_seed(self.seed, chunk_x, chunk_z);
                        let alias_map = crate::world::structure::jigsaw::resolve_aliases(
                            &jigsaw_config.pool_aliases,
                            &mut alias_rng,
                        );

                        let mut assembly_rng = LegacyRandom::from_seed(0);
                        assembly_rng.set_large_feature_seed(self.seed, chunk_x, chunk_z);

                        let mut get_height_fn = |x: i32, z: i32| -> i32 {
                            // Vanilla creates a fresh NoiseChunk(1) per query. The aquifer
                            // is created at the CHUNK position (16-block aligned), not the
                            // cell position — see NoiseChunk line 137-139:
                            //   chunkX = SectionPos.blockToSectionCoord(chunkMinBlockX)
                            //   Aquifer.create(this, new ChunkPos(chunkX, chunkZ), ...)
                            // Vanilla: NoiseChunk at cell-aligned (r, s), aquifer at ChunkPos
                            let cw = N::Settings::CELL_WIDTH;
                            let cell_x = x.div_euclid(cw) * cw; // firstBlockX
                            let cell_z = z.div_euclid(cw) * cw; // firstBlockZ
                            let aq_chunk_x = (cell_x >> 4) * 16; // ChunkPos from cell pos
                            let aq_chunk_z = (cell_z >> 4) * 16;
                            let aq_cache = N::ColumnCache::default();
                            let mut fresh_aq = Aquifer::<N>::new(
                                aq_chunk_x,
                                aq_chunk_z,
                                N::Settings::MIN_Y,
                                N::Settings::HEIGHT,
                                &self.splitter,
                                &self.noises,
                                aq_cache,
                            );
                            // Init cache at the CHUNK containing the query (same grid
                            // as chunk generation uses for this position)
                            let mut fresh_cache = N::ColumnCache::default();
                            fresh_cache.init_grid(aq_chunk_x, aq_chunk_z, &self.noises);
                            iterate_noise_column_with_aquifer::<N>(
                                &mut fresh_cache,
                                &self.noises,
                                &mut fresh_aq,
                                x,
                                z,
                                false,
                            )
                        };

                        let result = crate::world::structure::jigsaw::assemble(
                            jigsaw_config,
                            &mut assembly_rng,
                            chunk_x,
                            chunk_z,
                            &self.template_pools,
                            &self.templates,
                            &alias_map,
                            &mut get_height_fn,
                            N::Settings::MIN_Y,
                            N::Settings::MIN_Y + N::Settings::HEIGHT,
                        );

                        if let Some(assembly) = result
                            && !assembly.pieces.is_empty() {
                                // Biome check at the stub position
                                let (bx, by, bz) = assembly.biome_check_pos;
                                let biome = sampler.sample(bx >> 2, by >> 2, bz >> 2);
                                if candidate.allowed_biomes.contains(&biome.key) {
                                    // Success — create the structure start
                                    let pieces = assembly
                                        .pieces
                                        .into_iter()
                                        .map(|pp| StructurePiece {
                                            piece_type: Identifier::new_static(
                                                "minecraft",
                                                "jigsaw",
                                            ),
                                            bounding_box: pp.bounding_box,
                                            gen_depth: pp.depth,
                                            orientation: None,
                                            nbt_data: Vec::new(),
                                            ground_level_delta: pp.ground_level_delta,
                                            junctions: pp.junctions,
                                        })
                                        .collect();
                                    let start = StructureStart {
                                        structure: candidate.structure.clone(),
                                        chunk_pos: steel_utils::ChunkPos::new(chunk_x, chunk_z),
                                        references: 0,
                                        pieces,
                                        bb_inflate: terrain_adapt_inflate(&candidate.structure),
                                    };
                                    chunk
                                        .structure_starts_mut()
                                        .insert(candidate.structure.clone(), start);
                                    break; // Done with this set
                                }
                            }

                        // Assembly failed or biome mismatch — retry with next candidate
                        total_weight -= candidate.weight;
                        remaining.remove(selected_idx);
                    }
                }
            }
        }
    }

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

        // Collect writes per (x,z) column and flush in batch to avoid per-block
        // write lock acquisition on sections.
        let mut pending_writes: Vec<(usize, usize, usize, BlockStateId)> = Vec::new();
        let mut prev_x: usize = usize::MAX;
        let mut prev_z: usize = usize::MAX;
        let sections = chunk.sections();

        noise_chunk.fill(
            noises,
            &mut column_cache,
            beard_opt,
            |local_x, world_y, local_z, density, interpolated, cache| {
                // Flush when we move to a new column
                if local_x != prev_x || local_z != prev_z {
                    if !pending_writes.is_empty() {
                        sections.write_block_batch(&pending_writes);
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
                    }
                    AquiferResult::Fluid(id) => {
                        pending_writes.push((local_x, relative_y, local_z, id));
                    }
                    AquiferResult::Air => {}
                }
            },
        );

        // Flush remaining writes
        if !pending_writes.is_empty() {
            sections.write_block_batch(&pending_writes);
        }
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

        let eroded_badlands_id = (*vanilla_biomes::ERODED_BADLANDS).id() as u16;
        let frozen_ocean_id = (*vanilla_biomes::FROZEN_OCEAN).id() as u16;
        let deep_frozen_ocean_id = (*vanilla_biomes::DEEP_FROZEN_OCEAN).id() as u16;

        // Pre-extract all biome palette values to avoid per-read section locking.
        let biome_data = chunk.sections().read_all_biomes();
        let section_count = chunk.sections().sections.len();

        let mut pending_writes: Vec<(usize, BlockStateId)> = Vec::new();
        let mut column_buf: Vec<BlockStateId> = Vec::new();

        for local_x in 0..16usize {
            for local_z in 0..16usize {
                let block_x = chunk_min_x + local_x as i32;
                let block_z = chunk_min_z + local_z as i32;

                // Start scanning from one above the highest non-air block
                let mut start_height = worldgen_surface.get_first_available(local_x, local_z);

                // Column-local Voronoi cache for fuzzed biome lookups
                let mut biome_col = FuzzedBiomeColumn::new(
                    &biome_data,
                    section_count,
                    self.biome_zoom_seed,
                    block_x,
                    block_z,
                    min_y,
                    chunk_quart_x,
                    chunk_quart_z,
                    neighbor_biomes,
                );

                // Eroded badlands extension: add terracotta pillars above surface
                let surface_biome_id = biome_col.get(start_height);
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

                // Snapshot the column once — avoids per-block section locking in the Y scan.
                // Taken after eroded_badlands_extension which may write blocks above the surface.
                chunk
                    .sections()
                    .read_column_into(local_x, local_z, &mut column_buf);

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

                // Steep condition: vanilla only checks south >= north + 4 and
                // west >= east + 4 (asymmetric, not absolute difference).
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
                        // Get biome via fuzzed BiomeManager lookup
                        let biome_id = biome_col.get(y);

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
                            pending_writes.push((relative_y, new_block));
                        }
                    }
                }

                // Flush batched writes — holds each section's write guard once
                if !pending_writes.is_empty() {
                    chunk
                        .sections()
                        .write_column_blocks(local_x, local_z, &pending_writes);
                    chunk.mark_dirty();
                }

                // Frozen ocean iceberg extension: add packed ice and snow
                if surface_biome_id == frozen_ocean_id || surface_biome_id == deep_frozen_ocean_id {
                    self.surface_system.frozen_ocean_extension(
                        chunk,
                        surface_biome_id,
                        local_x,
                        local_z,
                        block_x,
                        block_z,
                        start_height,
                        min_surface_level,
                        min_y,
                    );
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
    neighbor_biomes: &'a dyn Fn(i32, i32, i32) -> u16,
    cached_parent_y: i32,
    /// Per-candidate cached values: (`fy`, `xz_partial_distance`).
    candidates: [(f64, f64); 8],
    /// Precomputed `lcg_next(seed, parent_x)` and `lcg_next(seed, parent_x + 1)`.
    rval_after_cx: [i64; 2],
}

impl<'a> FuzzedBiomeColumn<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        biome_data: &'a [u16],
        section_count: usize,
        biome_zoom_seed: i64,
        block_x: i32,
        block_z: i32,
        min_y: i32,
        chunk_quart_x: i32,
        chunk_quart_z: i32,
        neighbor_biomes: &'a dyn Fn(i32, i32, i32) -> u16,
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
    #[allow(clippy::similar_names)]
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

        let biome_qx = if (min_i & 4) == 0 {
            self.parent_x
        } else {
            self.parent_x + 1
        };
        let biome_qy = if (min_i & 2) == 0 {
            parent_y
        } else {
            parent_y + 1
        };
        let biome_qz = if (min_i & 1) == 0 {
            self.parent_z
        } else {
            self.parent_z + 1
        };

        let in_chunk = biome_qx >= self.chunk_quart_x
            && biome_qx < self.chunk_quart_x + 4
            && biome_qz >= self.chunk_quart_z
            && biome_qz < self.chunk_quart_z + 4;

        if in_chunk {
            let min_qy = self.min_y >> 2;
            let total_quarts_y = self.section_count * 4;
            let local_qx = (biome_qx - self.chunk_quart_x) as usize;
            let local_qz = (biome_qz - self.chunk_quart_z) as usize;
            let qy_in_chunk = (biome_qy - min_qy).clamp(0, total_quarts_y as i32 - 1) as usize;
            let section_idx = qy_in_chunk / 4;
            let local_qy = qy_in_chunk % 4;
            self.biome_data[section_idx * 64 + local_qy * 16 + local_qz * 4 + local_qx]
        } else {
            (self.neighbor_biomes)(biome_qx, biome_qy, biome_qz)
        }
    }
}
