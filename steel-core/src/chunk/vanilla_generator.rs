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
use steel_utils::{BlockStateId, Identifier};

use crate::chunk::aquifer::{Aquifer, AquiferResult, preliminary_surface_level};
use crate::chunk::beardifier::Beardifier;
use crate::chunk::chunk_access::ChunkAccess;
use crate::chunk::chunk_generator::ChunkGenerator;
use crate::chunk::heightmap::HeightmapType;
use crate::chunk::noise_chunk::NoiseChunk;
use crate::chunk::ore_veinifier::OreVeinifier;
use crate::chunk::surface_system::SurfaceSystem;
use crate::world::structure::placement::{
    PlacementKind, StructureSelectionEntry, StructureSet, generate_ring_positions,
    load_vanilla_structure_sets,
};
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
    /// Registry of `Structure` impls keyed by `structure_type` string
    /// (e.g. `"minecraft:nether_fossil"`). Entries here take precedence over
    /// the old per-type match arms in `create_structures` — as structures are
    /// ported to the trait they're added here and their match arms deleted.
    structures: rustc_hash::FxHashMap<
        String,
        Box<dyn crate::world::structure::Structure<N>>,
    >,
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

        let mut structures: rustc_hash::FxHashMap<
            String,
            Box<dyn crate::world::structure::Structure<N>>,
        > = rustc_hash::FxHashMap::default();
        structures.insert(
            "minecraft:nether_fossil".to_string(),
            Box::new(crate::world::structure::nether_fossil::NetherFossilStructure),
        );
        structures.insert(
            "minecraft:fortress".to_string(),
            Box::new(crate::world::structure::fortress::NetherFortressStructure),
        );
        structures.insert(
            "minecraft:end_city".to_string(),
            Box::new(crate::world::structure::end_city::EndCityStructure),
        );
        structures.insert(
            "minecraft:woodland_mansion".to_string(),
            Box::new(crate::world::structure::mansion::WoodlandMansionStructure),
        );
        structures.insert(
            "minecraft:ocean_monument".to_string(),
            Box::new(crate::world::structure::ocean_monument::OceanMonumentStructure),
        );
        structures.insert(
            "minecraft:mineshaft".to_string(),
            Box::new(crate::world::structure::mineshaft::MineshaftStructure),
        );
        {
            use crate::world::structure::single_piece::{
                BuriedTreasureStructure, SinglePieceStructure,
            };
            structures.insert(
                "minecraft:desert_pyramid".to_string(),
                Box::new(SinglePieceStructure {
                    size: (21, 15, 21),
                    piece_id: "tedp",
                    require_above_sea: true,
                }),
            );
            structures.insert(
                "minecraft:jungle_temple".to_string(),
                Box::new(SinglePieceStructure {
                    size: (12, 10, 15),
                    piece_id: "tejp",
                    require_above_sea: true,
                }),
            );
            structures.insert(
                "minecraft:swamp_hut".to_string(),
                Box::new(SinglePieceStructure {
                    size: (7, 7, 9),
                    piece_id: "tesh",
                    require_above_sea: false,
                }),
            );
            structures.insert(
                "minecraft:buried_treasure".to_string(),
                Box::new(BuriedTreasureStructure),
            );
        }
        structures.insert(
            "minecraft:shipwreck".to_string(),
            Box::new(crate::world::structure::shipwreck::ShipwreckStructure),
        );
        structures.insert(
            "minecraft:igloo".to_string(),
            Box::new(crate::world::structure::igloo::IglooStructure),
        );
        structures.insert(
            "minecraft:ocean_ruin".to_string(),
            Box::new(crate::world::structure::ocean_ruin::OceanRuinStructure),
        );
        structures.insert(
            "minecraft:stronghold".to_string(),
            Box::new(crate::world::structure::stronghold::StrongholdStructure),
        );
        structures.insert(
            "minecraft:ruined_portal".to_string(),
            Box::new(crate::world::structure::ruined_portal::RuinedPortalStructure),
        );

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
            structures,
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
pub(crate) fn iterate_noise_column_with_aquifer<N: DimensionNoises>(
    cache: &mut N::ColumnCache,
    noises: &N,
    aquifer: &mut Aquifer<N>,
    block_x: i32,
    block_z: i32,
    ocean_floor: bool,
) -> i32 {
    let max_y = N::Settings::MIN_Y + N::Settings::HEIGHT - 1;
    iterate_noise_column_capped::<N>(cache, noises, aquifer, block_x, block_z, max_y, ocean_floor)
}

/// `getBaseHeight(WORLD_SURFACE_WG)`-compatible height scan. Uses
/// `preliminary_surface_level + 16` as an upper bound to avoid scanning empty
/// upper atmosphere, and the cell-based iterator so 8-corner density
/// evaluations are shared across Y values in each cell.
///
/// Exposed for `GenerationContext`.
pub(crate) fn column_base_height<N: DimensionNoises>(
    cache: &mut N::ColumnCache,
    noises: &N,
    aquifer: &mut Aquifer<N>,
    x: i32,
    z: i32,
    ocean_floor: bool,
) -> i32 {
    let estimate = preliminary_surface_level::<N>(noises, cache, x, z);
    let max_y = (estimate + 16).min(N::Settings::MIN_Y + N::Settings::HEIGHT - 1);
    iterate_noise_column_capped::<N>(cache, noises, aquifer, x, z, max_y, ocean_floor)
}

/// Single-point `getInterpolatedDensity`. Exposed for `GenerationContext`.
pub(crate) fn column_interpolated_density<N: DimensionNoises>(
    cache: &mut N::ColumnCache,
    noises: &N,
    x: i32,
    y: i32,
    z: i32,
    cell_w: i32,
    cell_h: i32,
) -> f64 {
    interpolated_density::<N>(cache, noises, x, y, z, cell_w, cell_h)
}

/// Same as `iterate_noise_column_with_aquifer` but only scans Y values up to
/// `max_y_inclusive`. Used by `base_height` with an estimate from
/// `preliminary_surface_level + 16` to skip empty upper atmosphere — reducing
/// cell-corner density evaluations from O(height) to O(estimate_depth).
fn iterate_noise_column_capped<N: DimensionNoises>(
    cache: &mut N::ColumnCache,
    noises: &N,
    aquifer: &mut Aquifer<N>,
    block_x: i32,
    block_z: i32,
    max_y_inclusive: i32,
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

    // Topmost cell containing max_y_inclusive.
    let max_cell_y_idx = {
        let raw = max_y_inclusive.div_euclid(cell_h) - cell_min_y;
        raw.clamp(0, cell_count_y - 1)
    };
    // Top Y-within-cell for the topmost cell.
    let top_cell_top_y_in_cell = (max_y_inclusive - (cell_min_y + max_cell_y_idx) * cell_h)
        .clamp(0, cell_h - 1);

    for cell_y_idx in (0..=max_cell_y_idx).rev() {
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

        // For the topmost cell, start from the Y within cell that corresponds
        // to `max_y_inclusive`. For lower cells, start at cell_h - 1.
        let top_y_in_cell = if cell_y_idx == max_cell_y_idx {
            top_cell_top_y_in_cell
        } else {
            cell_h - 1
        };

        // Iterate Y within cell from top to bottom
        for y_in_cell in (0..=top_y_in_cell).rev() {
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
        // Pre-populate the grid: most can_generate height queries land inside
        // the chunk's quart grid, which makes them O(1) lookups instead of
        // on-the-fly density evaluations.
        height_cache.init_grid(chunk_min_x, chunk_min_z, &self.noises);
        let sea_level = N::Settings::SEA_LEVEL;

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

        // Aquifer-aware height scan. Uses preliminary_surface_level + 16 as the
        // upper bound to skip empty upper atmosphere, and the cell-based
        // iterator so corner-density evaluations are shared across Y values
        // in each cell.
        let base_height = |cache: &mut N::ColumnCache,
                           noises: &N,
                           aquifer: &mut Aquifer<N>,
                           x: i32,
                           z: i32,
                           ocean_floor: bool|
         -> i32 {
            let estimate = preliminary_surface_level::<N>(noises, cache, x, z);
            let max_y = (estimate + 16).min(N::Settings::MIN_Y + N::Settings::HEIGHT - 1);
            iterate_noise_column_capped::<N>(cache, noises, aquifer, x, z, max_y, ocean_floor)
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
            /// Non-jigsaw: selection already done. `cached_stub` holds the
            /// pieces produced by `Structure::find_generation_point`; `None`
            /// means the entry is a legacy/unknown type and piece-gen emits
            /// an empty start.
            Single {
                structure: Identifier,
                cached_stub: Option<crate::world::structure::GenerationStub>,
            },
            /// Jigsaw: needs assembly + biome check with retry in post-processing.
            Jigsaw(Vec<StructureSelectionEntry>),
        }

        /// Result of a non-jigsaw entry-evaluation attempt.
        enum CanGen {
            No,
            /// Legacy path (currently only `minecraft:jigsaw` biome check — the
            /// outer `is_jigsaw_set` branch handles those separately, so this
            /// fires only for unknown/default types).
            Legacy,
            /// Structure trait ran successfully; stub can be reused in piece-gen.
            Trait(crate::world::structure::GenerationStub),
        }
        let mut selected_sets: Vec<SelectedSet> = Vec::new();

        // Evaluates a non-jigsaw entry. For registered structures runs
        // `find_generation_point` and returns the stub so piece-gen can reuse
        // it; for unknown types falls back to a center+surface_y biome check.
        // (Jigsaw entries take a separate path via `SelectedSet::Jigsaw`.)
        let mut can_generate = |entry: &StructureSelectionEntry| -> CanGen {
            if let Some(structure_impl) = self.structures.get(&entry.structure_type) {
                let mut reg_rng = LegacyRandom::from_seed(0);
                reg_rng.set_large_feature_seed(self.seed, chunk_x, chunk_z);
                let mut ctx = crate::world::structure::GenerationContext::<'_, '_, N> {
                    seed: self.seed,
                    chunk_x,
                    chunk_z,
                    chunk_min_x,
                    chunk_min_z,
                    center_block_x,
                    center_block_z,
                    sea_level,
                    surface_y,
                    noises: &self.noises,
                    splitter: &self.splitter,
                    templates: &self.templates,
                    biome_sampler: &mut sampler,
                    height_cache: &mut height_cache,
                    aquifer: &mut aquifer,
                };
                return match structure_impl.find_generation_point(&mut ctx, entry, &mut reg_rng) {
                    Some(stub) => CanGen::Trait(stub),
                    None => CanGen::No,
                };
            }

            // Unknown / legacy types — do a simple center + surface-Y biome check.
            tracing::warn!(
                "Unknown structure type {:?} for {}, using center biome check",
                entry.structure_type,
                entry.structure
            );
            let biome =
                sampler.sample(center_block_x >> 2, surface_y >> 2, center_block_z >> 2);
            if entry.allowed_biomes.contains(&biome.key) {
                CanGen::Legacy
            } else {
                CanGen::No
            }
        };

        for (set_key, set) in &self.structure_sets {
            let is_jigsaw_set = set
                .structures
                .iter()
                .any(|e| e.structure_type == "minecraft:jigsaw");

            // Placement check first — it's effectively free (a little RNG hashing)
            // and rejects ~1 - 1/spacing² of chunks. Running `can_generate` first
            // was catastrophic because `can_generate` for some structure types
            // (e.g. mineshaft) runs full piece generation just to compute a biome
            // check position.
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
                // Non-jigsaw: weighted selection. For registered structures
                // `can_generate` produces the final stub — stash it so piece-gen
                // doesn't re-run `find_generation_point`.
                let selected: Option<(&StructureSelectionEntry, Option<crate::world::structure::GenerationStub>)> =
                    if set.structures.len() == 1 {
                        let entry = &set.structures[0];
                        match can_generate(entry) {
                            CanGen::Trait(stub) => Some((entry, Some(stub))),
                            CanGen::Legacy => Some((entry, None)),
                            CanGen::No => None,
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
                            match can_generate(candidate) {
                                CanGen::Trait(stub) => {
                                    result = Some((candidate, Some(stub)));
                                    break;
                                }
                                CanGen::Legacy => {
                                    result = Some((candidate, None));
                                    break;
                                }
                                CanGen::No => {
                                    total_weight -= candidate.weight;
                                    remaining.remove(selected_idx);
                                }
                            }
                        }

                        result
                    };

                if let Some((selected, cached_stub)) = selected {
                    selected_sets.push(SelectedSet::Single {
                        structure: selected.structure.clone(),
                        cached_stub,
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
                    cached_stub,
                } => {
                    // Cached stub from the selection pass. For registered
                    // structures this is always `Some`; for unknown/legacy
                    // types it's `None` and we emit an empty start.
                    let pieces = cached_stub.map(|s| s.pieces).unwrap_or_default();

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
                            // Non-jigsaw entry in a mixed set (e.g. fortress in
                            // nether_complexes). Dispatch via the Structure registry.
                            let stub_opt: Option<crate::world::structure::GenerationStub> = {
                                if let Some(structure_impl) =
                                    self.structures.get(&candidate.structure_type)
                                {
                                    let mut reg_rng = LegacyRandom::from_seed(0);
                                    reg_rng.set_large_feature_seed(
                                        self.seed, chunk_x, chunk_z,
                                    );
                                    let mut ctx = crate::world::structure::GenerationContext::<
                                        '_, '_, N,
                                    > {
                                        seed: self.seed,
                                        chunk_x,
                                        chunk_z,
                                        chunk_min_x,
                                        chunk_min_z,
                                        center_block_x,
                                        center_block_z,
                                        sea_level,
                                        surface_y,
                                        noises: &self.noises,
                                        splitter: &self.splitter,
                                        templates: &self.templates,
                                        biome_sampler: &mut sampler,
                                        height_cache: &mut height_cache,
                                        aquifer: &mut aquifer,
                                    };
                                    structure_impl.find_generation_point(
                                        &mut ctx, candidate, &mut reg_rng,
                                    )
                                } else {
                                    None
                                }
                            };
                            if let Some(stub) = stub_opt {
                                let start = StructureStart {
                                    structure: candidate.structure.clone(),
                                    chunk_pos: steel_utils::ChunkPos::new(chunk_x, chunk_z),
                                    references: 0,
                                    pieces: stub.pieces,
                                    bb_inflate: terrain_adapt_inflate(&candidate.structure),
                                };
                                chunk
                                    .structure_starts_mut()
                                    .insert(candidate.structure.clone(), start);
                                break;
                            }
                            // Retry with next candidate.
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
