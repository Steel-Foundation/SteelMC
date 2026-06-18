//! Chunk generation stage regression test.
//!
//! Verifies that Steel's chunk generation matches vanilla Minecraft at each stage
//! by comparing MD5 hashes of block data. When a mismatch is found and binary
//! reference data is available, shows exact block-level diffs.
//!
//! Tests all dimensions (overworld, nether, end) using the new JSON format
//! with a `dimensions` wrapper.

use std::env;
use std::fmt::Write;
use std::fs;
use std::io::{BufReader, Cursor, Read as IoRead};
use std::mem;
use std::sync::{Arc, Weak};

use flate2::read::GzDecoder;
use rustc_hash::{FxBuildHasher, FxHashMap, FxHashSet};
use serde::Deserialize;
use steel_core::chunk::chunk_access::{ChunkAccess, ChunkStatus};
use steel_core::chunk::chunk_generation_task::StaticCache2D;
use steel_core::chunk::chunk_holder::ChunkHolder;
use steel_core::chunk::chunk_pyramid::GENERATION_PYRAMID;
use steel_core::chunk::chunk_ticket_manager::{ChunkTicketLevel, MAX_VIEW_DISTANCE};
use steel_core::chunk::proto_chunk::ProtoChunk;
use steel_core::chunk::section::{ChunkSection, Sections};
use steel_core::level_data::WorldGenerationSettings;
use steel_core::world::{World, WorldConfig, WorldStorageConfig};
use steel_core::worldgen::{ChunkGenerator, ChunkGeneratorType};
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::structure::TerrainAdjustment;
use steel_registry::{dimension_type::DimensionTypeRef, vanilla_dimension_types};
use steel_utils::types::{Difficulty, GameType};
use steel_utils::{ChunkPos, Identifier};
use steel_worldgen::noise::Beardifier;
use steel_worldgen::structure::StructureStart;
use tokio::runtime::Runtime;
use toml::map::Map;

#[derive(Deserialize, Debug)]
struct ChunkStageEntry {
    x: i32,
    z: i32,
    stages: FxHashMap<String, String>,
}

#[derive(Deserialize, Debug)]
struct DimensionData {
    chunks: Vec<ChunkStageEntry>,
}

#[derive(Deserialize, Debug)]
struct ChunkStageHashesJson {
    seed: u64,
    chunk_generation_order: String,
    #[serde(default)]
    feature_hash_capture: Option<String>,
    #[serde(default)]
    hashset_iteration_order: Option<String>,
    dimensions: FxHashMap<String, DimensionData>,
}

/// Stages to verify in vanilla generation order.
const STAGES: &[&str] = &[
    "minecraft:noise",
    "minecraft:surface",
    "minecraft:carvers",
    "minecraft:features",
];

/// Match the extractor run's structure setting.
///
/// Set this to `false` when the vanilla fixture was produced with
/// `-DMC_DEBUG_DISABLE_STRUCTURES=true`.
const GENERATE_STRUCTURES: bool = true;

/// Max block-level diffs to show per chunk before truncating.
const MAX_DIFFS_PER_CHUNK: usize = 30;

/// Set specific chunk coordinates to test only those chunks.
/// When non-empty, only these chunks are generated and checked (ignores the JSON list).
/// Example: &[(24, 35)] to debug a single failing chunk.
const DEBUG_CHUNKS: &[(i32, i32)] = &[];
const DEBUG_CLUSTER_ENV: &str = "STEEL_HASH_DEBUG_CLUSTER";
const DEBUG_CHUNK_ENV: &str = "STEEL_HASH_DEBUG_CHUNK";
const DEBUG_DIMENSION_ENV: &str = "STEEL_HASH_DEBUG_DIMENSION";
const DEBUG_STAGE_ENV: &str = "STEEL_HASH_DEBUG_STAGE";
const DEBUG_STOP_AFTER_FIRST_MISMATCH_ENV: &str = "STEEL_HASH_STOP_AFTER_FIRST_MISMATCH";

const FEATURE_STAGE: &str = "minecraft:features";
const CHUNK_GENERATION_ORDER_X_Z_ASCENDING: &str = "x_z_ascending";
const FEATURE_HASH_CAPTURE_AFTER_ALL_READY: &str = "after_all_tracked_features_ready";
const HASHSET_ITERATION_ORDER_INSERTION: &str = "insertion_order";

fn load_expected_hashes() -> ChunkStageHashesJson {
    let json_str = include_str!("../test_assets/chunk_stage_hashes.json");
    serde_json::from_str(json_str).expect("Failed to parse chunk_stage_hashes.json")
}

fn sorted_positions(positions: &FxHashSet<(i32, i32)>) -> Vec<(i32, i32)> {
    let mut positions = positions.iter().copied().collect::<Vec<_>>();
    positions.sort_unstable();
    positions
}

fn debug_chunk_filter() -> Option<FxHashSet<(i32, i32)>> {
    let mut chunks = FxHashSet::default();
    chunks.extend(DEBUG_CHUNKS.iter().copied());

    if let Ok(chunk) = env::var(DEBUG_CHUNK_ENV) {
        let Some((x, z)) = chunk.split_once(',') else {
            panic!("{DEBUG_CHUNK_ENV} must be formatted as '<chunk_x>,<chunk_z>'");
        };
        let Ok(chunk_x) = x.parse::<i32>() else {
            panic!("{DEBUG_CHUNK_ENV} chunk_x is not an i32: {x}");
        };
        let Ok(chunk_z) = z.parse::<i32>() else {
            panic!("{DEBUG_CHUNK_ENV} chunk_z is not an i32: {z}");
        };
        chunks.insert((chunk_x, chunk_z));
    }

    if let Ok(cluster) = env::var(DEBUG_CLUSTER_ENV) {
        let Some((x, z)) = cluster.split_once(',') else {
            panic!("{DEBUG_CLUSTER_ENV} must be formatted as '<chunk_x>,<chunk_z>'");
        };
        let Ok(origin_x) = x.parse::<i32>() else {
            panic!("{DEBUG_CLUSTER_ENV} chunk_x is not an i32: {x}");
        };
        let Ok(origin_z) = z.parse::<i32>() else {
            panic!("{DEBUG_CLUSTER_ENV} chunk_z is not an i32: {z}");
        };

        for dx in 0..10 {
            for dz in 0..10 {
                chunks.insert((origin_x + dx, origin_z + dz));
            }
        }
    }

    (!chunks.is_empty()).then_some(chunks)
}

fn debug_dimension_filter() -> Option<String> {
    env::var(DEBUG_DIMENSION_ENV)
        .ok()
        .filter(|dimension| !dimension.is_empty())
}

fn debug_stage_filter() -> Option<String> {
    env::var(DEBUG_STAGE_ENV)
        .ok()
        .filter(|stage| !stage.is_empty())
}

fn empty_proto_chunk(
    pos: (i32, i32),
    section_count: usize,
    min_y: i32,
    height: i32,
) -> ChunkAccess {
    let sections: Box<[ChunkSection]> = (0..section_count)
        .map(|_| ChunkSection::new_empty())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let proto = ProtoChunk::new(
        Sections::from_owned(sections),
        ChunkPos::new(pos.0, pos.1),
        min_y,
        height,
        Weak::new(),
    );
    ChunkAccess::Proto(proto)
}

fn chunk_or_panic(chunks: &FxHashMap<(i32, i32), ChunkAccess>, pos: (i32, i32)) -> &ChunkAccess {
    match chunks.get(&pos) {
        Some(chunk) => chunk,
        None => panic!("Missing test chunk ({}, {})", pos.0, pos.1),
    }
}

fn create_test_world(
    dim_key: &str,
    dim_type: DimensionTypeRef,
    seed: u64,
    generator: Arc<ChunkGeneratorType>,
) -> Arc<World> {
    let runtime = Arc::new(Runtime::new().expect("failed to create chunk-stage hash test runtime"));
    let generation_pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .thread_name(|index| format!("chunk-stage-hashes-{index}"))
            .build()
            .expect("failed to create chunk-stage hash test rayon pool"),
    );
    let dim_short = dim_key.strip_prefix("minecraft:").unwrap_or(dim_key);
    let empty_config = toml::Value::Table(Map::new());
    let generation_settings = WorldGenerationSettings::from_generator_config(
        Identifier::new(Identifier::VANILLA_NAMESPACE, dim_short.to_owned()),
        &empty_config,
        dim_type.key.clone(),
        dim_type.min_y,
        dim_type.height,
    );
    let sea_level = match dim_key {
        "minecraft:the_nether" => 32,
        "minecraft:the_end" => 0,
        _ => 63,
    };

    runtime
        .block_on(World::new_with_config(
            runtime.clone(),
            Identifier::new(Identifier::VANILLA_NAMESPACE, dim_short.to_owned()),
            dim_type,
            seed as i64,
            WorldConfig {
                storage: WorldStorageConfig::RamOnly,
                level_data_path: None,
                generator,
                generation_settings,
                view_distance: 2,
                simulation_distance: 2,
                compression: None,
                is_flat: false,
                sea_level,
                default_gamemode: GameType::Survival,
                difficulty: Difficulty::Normal,
            },
            generation_pool,
        ))
        .expect("failed to create chunk-stage hash test world")
}

fn build_feature_holders(
    chunks: FxHashMap<(i32, i32), ChunkAccess>,
    carver_positions: &FxHashSet<(i32, i32)>,
    min_y: i32,
    height: i32,
) -> FxHashMap<(i32, i32), Arc<ChunkHolder>> {
    let mut holders = FxHashMap::with_capacity_and_hasher(chunks.len(), FxBuildHasher);
    for (pos, chunk) in chunks {
        let holder = Arc::new(ChunkHolder::new(
            ChunkPos::new(pos.0, pos.1),
            ChunkTicketLevel::for_full_chunk_radius(MAX_VIEW_DISTANCE),
            None,
            min_y,
            height,
        ));
        let status = if carver_positions.contains(&pos) {
            ChunkStatus::Carvers
        } else {
            ChunkStatus::StructureStarts
        };
        if let ChunkAccess::Proto(proto) = &chunk {
            proto.set_status(status);
        }
        holder.insert_chunk(chunk, status);
        holders.insert(pos, holder);
    }
    holders
}

fn compute_block_hash(sections: &Sections) -> String {
    let mut ctx = md5::Context::new();

    for section_holder in &sections.sections {
        let section = section_holder.read();
        // Match vanilla's `LevelChunkSection.hasOnlyAir()` — which returns
        // true when `nonEmptyBlockCount == 0`, i.e. every block is air /
        // cave_air / void_air. Steel's palette-level `has_only_air()` doesn't
        // treat a heterogeneous cave_air-only palette as "empty", so we scan
        // manually to match the extractor's shortcut.
        let mut all_air = true;
        'scan: for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    if !section.states.get(x, y, z).is_air() {
                        all_air = false;
                        break 'scan;
                    }
                }
            }
        }
        if all_air {
            ctx.consume([0u8]);
        } else {
            for y in 0..16 {
                for z in 0..16 {
                    for x in 0..16 {
                        let state = section.states.get(x, y, z);
                        let state_id = u32::from(state.0);
                        ctx.consume([(state_id >> 24) as u8]);
                        ctx.consume([(state_id >> 16) as u8]);
                        ctx.consume([(state_id >> 8) as u8]);
                        ctx.consume([state_id as u8]);
                    }
                }
            }
        }
    }

    format!("{:x}", ctx.finalize())
}

/// Per-chunk reference block data from the extractor binary.
struct ChunkBlockData {
    /// Sections, each None (all air) or Some(4096 state IDs in YZX order).
    sections: Vec<Option<Vec<i32>>>,
}

/// Loads binary reference block data for a given stage and dimension.
///
/// Binary format (gzip compressed, all integers big-endian):
///   `chunk_count`: i32
///   For each chunk:
///     `chunk_x`: i32
///     `chunk_z`: i32
///     `section_count`: i32
///     For each section:
///       `has_data`: u8
///       if `has_data` == 1: `state_ids`: [i32; 4096]
fn load_reference_blocks(
    stage: &str,
    dim_short: &str,
) -> Option<FxHashMap<(i32, i32), ChunkBlockData>> {
    let short_name = stage.strip_prefix("minecraft:").unwrap_or(stage);
    let path = format!(
        "{}/test_assets/chunk_stage_{dim_short}_{short_name}_blocks.bin.gz",
        env!("CARGO_MANIFEST_DIR"),
    );
    let compressed = fs::read(&path).ok()?;

    let decoder = GzDecoder::new(Cursor::new(compressed));
    let mut buf = Vec::new();
    BufReader::new(decoder).read_to_end(&mut buf).ok()?;

    let mut pos = 0;

    let read_i32 = |pos: &mut usize| -> i32 {
        let val = i32::from_be_bytes(
            buf[*pos..*pos + 4]
                .try_into()
                .expect("slice should be 4 bytes"),
        );
        *pos += 4;
        val
    };

    let chunk_count = read_i32(&mut pos) as usize;
    let mut map = FxHashMap::with_capacity_and_hasher(chunk_count, FxBuildHasher);

    for _ in 0..chunk_count {
        let cx = read_i32(&mut pos);
        let cz = read_i32(&mut pos);
        let section_count = read_i32(&mut pos) as usize;
        let mut sections = Vec::with_capacity(section_count);

        for _ in 0..section_count {
            let has_data = buf[pos];
            pos += 1;
            if has_data == 0 {
                sections.push(None);
            } else {
                let mut state_ids = Vec::with_capacity(4096);
                for _ in 0..4096 {
                    state_ids.push(read_i32(&mut pos));
                }
                sections.push(Some(state_ids));
            }
        }

        map.insert((cx, cz), ChunkBlockData { sections });
    }

    Some(map)
}

/// Format a state ID as "id (`block_name`[props])" for human-readable output.
fn describe_state(state_id: i32) -> String {
    use steel_registry::REGISTRY;
    use steel_utils::types::BlockStateId;

    let bsid = BlockStateId(state_id as u16);
    let Some(block) = REGISTRY.blocks.by_state_id(bsid) else {
        return format!("{state_id} (unknown)");
    };
    let props = REGISTRY.blocks.get_properties(bsid);
    if props.is_empty() {
        format!("{state_id} ({})", block.key)
    } else {
        let prop_str: Vec<_> = props.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("{state_id} ({}[{}])", block.key, prop_str.join(","))
    }
}

struct BlockDiff {
    x: usize,
    y: i32,
    z: usize,
    vanilla: i32,
    steel: i32,
}

/// Compare a chunk's sections against reference data, returning block-level diffs.
fn diff_chunk(sections: &Sections, reference: &ChunkBlockData, min_y: i32) -> Vec<BlockDiff> {
    let mut diffs = Vec::new();

    for (si, section_holder) in sections.sections.iter().enumerate() {
        let section = section_holder.read();
        let ref_section = reference.sections.get(si);
        let section_base_y = min_y + (si as i32) * 16;

        match ref_section {
            Some(Some(ref_ids)) => {
                if section.states.has_only_air() {
                    // Steel says all air, vanilla has data
                    for (idx, &vanilla_id) in ref_ids.iter().enumerate() {
                        if vanilla_id != 0 {
                            let y_local = idx / 256;
                            let z = (idx % 256) / 16;
                            let x = idx % 16;
                            diffs.push(BlockDiff {
                                x,
                                y: section_base_y + y_local as i32,
                                z,
                                vanilla: vanilla_id,
                                steel: 0,
                            });
                        }
                    }
                } else {
                    for y_local in 0..16usize {
                        for z in 0..16usize {
                            for x in 0..16usize {
                                let idx = y_local * 256 + z * 16 + x;
                                let vanilla_id = ref_ids[idx];
                                let steel_id =
                                    u32::from(section.states.get(x, y_local, z).0) as i32;
                                if vanilla_id != steel_id {
                                    diffs.push(BlockDiff {
                                        x,
                                        y: section_base_y + y_local as i32,
                                        z,
                                        vanilla: vanilla_id,
                                        steel: steel_id,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            Some(None) | None => {
                // Vanilla says all air (or section missing). Check if Steel also has air.
                if !section.states.has_only_air() {
                    for y_local in 0..16usize {
                        for z in 0..16usize {
                            for x in 0..16usize {
                                let steel_id =
                                    u32::from(section.states.get(x, y_local, z).0) as i32;
                                if steel_id != 0 {
                                    diffs.push(BlockDiff {
                                        x,
                                        y: section_base_y + y_local as i32,
                                        z,
                                        vanilla: 0,
                                        steel: steel_id,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    diffs
}

/// Format block diffs into a human-readable report for a single chunk.
fn format_chunk_diffs(diffs: &[BlockDiff], chunk_x: i32, chunk_z: i32, min_y: i32) -> String {
    let mut msg = format!(
        "  Chunk ({chunk_x:3},{chunk_z:3}): {} block differences\n",
        diffs.len()
    );

    // Group by section
    let mut by_section: FxHashMap<i32, Vec<&BlockDiff>> = FxHashMap::default();
    for d in diffs {
        let section_idx = (d.y - min_y) / 16;
        by_section.entry(section_idx).or_default().push(d);
    }

    let mut section_indices: Vec<_> = by_section.keys().copied().collect();
    section_indices.sort_unstable();

    let mut shown = 0;
    for si in section_indices {
        let section_diffs = &by_section[&si];
        let section_base = min_y + si * 16;
        let _ = writeln!(
            msg,
            "    Section {si} (y={section_base}..{}): {} differences",
            section_base + 15,
            section_diffs.len()
        );

        for d in section_diffs {
            if shown >= MAX_DIFFS_PER_CHUNK {
                let remaining = diffs.len() - shown;
                let _ = writeln!(msg, "      ... and {remaining} more");
                return msg;
            }
            let _ = writeln!(
                msg,
                "      ({:2},{:4},{:2}): vanilla={} steel={}",
                d.x,
                d.y,
                d.z,
                describe_state(d.vanilla),
                describe_state(d.steel),
            );
            shown += 1;
        }
    }

    msg
}

#[test]
#[ignore = "This test takes too long to run for normal testing; run with --release"]
fn chunk_stage_hashes() {
    use std::panic;
    use std::thread;

    // Run on a thread with a larger stack to avoid overflow in debug builds,
    // since pre-generating biome data for neighbor lookups increases stack usage.
    let result = thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(chunk_stage_hashes_inner)
        .expect("Failed to spawn test thread")
        .join();

    if let Err(payload) = result {
        panic::resume_unwind(payload);
    }
}

/// Dimension order for deterministic test output (`HashMap` iteration is unordered).
const DIMENSION_ORDER: &[&str] = &[
    "minecraft:overworld",
    "minecraft:the_nether",
    "minecraft:the_end",
];

/// Build a beardifier for `chunk` using `chunks` as the chunk source. Mirrors the
/// production logic in `worldgen::stages::noise` but reads from a `HashMap` instead
/// of a chunk cache.
fn build_test_beardifier(
    chunk: &ChunkAccess,
    chunks: &FxHashMap<(i32, i32), ChunkAccess>,
) -> Option<Beardifier> {
    let pos = chunk.pos();
    let chunk_x = pos.0.x;
    let chunk_z = pos.0.y;

    let references = chunk.structure_references();

    let mut source_positions: FxHashSet<ChunkPos> = FxHashSet::default();
    for source_chunks in references.values() {
        source_positions.extend(source_chunks.iter().copied());
    }
    if source_positions.is_empty() {
        return None;
    }

    let source_chunk_refs: Vec<&ChunkAccess> = source_positions
        .iter()
        .filter_map(|p| chunks.get(&(p.0.x, p.0.y)))
        .collect();
    let mut source_indices: FxHashMap<ChunkPos, usize> = FxHashMap::default();
    let mut starts_guards = Vec::with_capacity(source_chunk_refs.len());
    for source_chunk in &source_chunk_refs {
        let source_pos = source_chunk.pos();
        source_indices.insert(source_pos, starts_guards.len());
        starts_guards.push(source_chunk.structure_starts());
    }

    let mut starts: Vec<&StructureStart> = Vec::new();
    for (structure_id, source_chunks_ref) in references.iter() {
        for &source_pos in source_chunks_ref {
            let Some(&guard_index) = source_indices.get(&source_pos) else {
                continue;
            };
            let guard = &starts_guards[guard_index];
            if let Some(start) = guard.get(structure_id)
                && start.chunk_pos == source_pos
                && start.terrain_adjustment != TerrainAdjustment::None
            {
                starts.push(start);
            }
        }
    }

    if starts.is_empty() {
        return None;
    }

    let beardifier = Beardifier::for_structures_in_chunk(starts.iter().copied(), chunk_x, chunk_z);
    (!beardifier.is_empty()).then_some(beardifier)
}

#[expect(
    clippy::too_many_lines,
    clippy::similar_names,
    reason = "large test with many hash assertions"
)]
fn chunk_stage_hashes_inner() {
    use steel_core::behavior::init_behaviors;
    use steel_core::block_entity::init_block_entities;
    use steel_core::entity::init_entities;
    use steel_core::worldgen::{EndGenerator, NetherGenerator, OverworldGenerator};
    use steel_registry::{REGISTRY, Registry};
    use steel_worldgen::biomes::BiomeSourceKind;

    let mut registry = Registry::new_vanilla();
    registry.freeze();
    let _ = REGISTRY.init(registry);
    init_behaviors();
    init_block_entities();
    init_entities();

    let expected = load_expected_hashes();
    let seed = expected.seed;
    assert_eq!(seed, 13579, "Expected seed 13579");
    assert_eq!(
        expected.chunk_generation_order, CHUNK_GENERATION_ORDER_X_Z_ASCENDING,
        "chunk stage hash test only supports x/z ascending generation order"
    );
    let includes_features = STAGES.contains(&FEATURE_STAGE);
    assert!(
        !includes_features || STAGES.last().copied() == Some(FEATURE_STAGE),
        "features must remain the last checked stage because it consumes the local chunk map"
    );
    if includes_features {
        assert_eq!(
            expected.feature_hash_capture.as_deref(),
            Some(FEATURE_HASH_CAPTURE_AFTER_ALL_READY),
            "features stage hashes must be extracted after all tracked features are ready; rerun the extractor"
        );
        assert_eq!(
            expected.hashset_iteration_order.as_deref(),
            Some(HASHSET_ITERATION_ORDER_INSERTION),
            "features stage hashes must be extracted with deterministic insertion-order HashSet normalization; rerun the extractor"
        );
    }
    let feature_step = GENERATION_PYRAMID.get_step_to(ChunkStatus::Features);
    let feature_cache_radius = feature_step.direct_dependencies.get_radius() as i32;
    let feature_carver_radius = feature_step
        .direct_dependencies
        .get_radius_of(ChunkStatus::Carvers) as i32;
    let debug_dimension = debug_dimension_filter();
    let debug_stage = debug_stage_filter();
    let stop_after_first_mismatch = env::var_os(DEBUG_STOP_AFTER_FIRST_MISMATCH_ENV).is_some();

    for &dim_key in DIMENSION_ORDER {
        if debug_dimension
            .as_deref()
            .is_some_and(|filter| filter != dim_key)
        {
            continue;
        }
        let Some(dim_data) = expected.dimensions.get(dim_key) else {
            continue;
        };

        let dim_short = dim_key.strip_prefix("minecraft:").unwrap_or(dim_key);
        let dim_type = match dim_key {
            "minecraft:overworld" => &vanilla_dimension_types::OVERWORLD,
            "minecraft:the_nether" => &vanilla_dimension_types::THE_NETHER,
            "minecraft:the_end" => &vanilla_dimension_types::THE_END,
            _ => panic!("Unknown dimension: {dim_key}"),
        };

        let min_y = dim_type.min_y;
        let height = dim_type.height;
        let section_count = (height / 16) as usize;
        let min_qy = min_y >> 2;
        let total_quarts_y = (section_count * 4) as i32;

        let generator: Arc<ChunkGeneratorType> = Arc::new(match dim_key {
            "minecraft:overworld" => {
                let source = BiomeSourceKind::overworld(seed);
                ChunkGeneratorType::Overworld(OverworldGenerator::new(source, seed))
            }
            "minecraft:the_nether" => {
                let source = BiomeSourceKind::nether(seed);
                ChunkGeneratorType::Nether(NetherGenerator::new(source, seed))
            }
            "minecraft:the_end" => {
                let source = BiomeSourceKind::end(seed);
                ChunkGeneratorType::End(EndGenerator::new(source, seed))
            }
            _ => unreachable!(),
        });
        let feature_world = includes_features
            .then(|| create_test_world(dim_key, dim_type, seed, generator.clone()));
        let feature_context = feature_world
            .as_ref()
            .map(|world| world.chunk_map.world_gen_context.clone());

        eprintln!("{dim_key}");

        let debug_filter = debug_chunk_filter();
        let mut test_entries: Vec<&ChunkStageEntry> = if let Some(filter) = &debug_filter {
            dim_data
                .chunks
                .iter()
                .filter(|c| filter.contains(&(c.x, c.z)))
                .collect()
        } else {
            dim_data.chunks.iter().collect()
        };
        test_entries.sort_unstable_by_key(|entry| (entry.x, entry.z));
        let tracked_positions: FxHashSet<(i32, i32)> = test_entries
            .iter()
            .map(|entry| (entry.x, entry.z))
            .collect();

        // Pre-pass: replicate vanilla's STRUCTURE_STARTS → STRUCTURE_REFERENCES →
        // BIOMES → NOISE pipeline before the per-stage hash loop. The beardifier in
        // production reads structure starts from referenced neighbor chunks, so the
        // test must populate those references the same way `generate_references` does
        // in `worldgen::stages::structures`.

        // 17×17 around each test chunk feeds STRUCTURE_REFERENCES. Surface and
        // feature dependency chunks add their required biome rings below.
        let mut starts_positions: FxHashSet<(i32, i32)> =
            FxHashSet::with_capacity_and_hasher(test_entries.len() * 289, FxBuildHasher);
        let mut biome_positions: FxHashSet<(i32, i32)> = FxHashSet::default();
        let mut feature_carver_positions: FxHashSet<(i32, i32)> = FxHashSet::default();
        for entry in &test_entries {
            if includes_features {
                for dx in -feature_cache_radius..=feature_cache_radius {
                    for dz in -feature_cache_radius..=feature_cache_radius {
                        starts_positions.insert((entry.x + dx, entry.z + dz));
                    }
                }
                for dx in -feature_carver_radius..=feature_carver_radius {
                    for dz in -feature_carver_radius..=feature_carver_radius {
                        feature_carver_positions.insert((entry.x + dx, entry.z + dz));
                    }
                }
            }
            for dx in -1i32..=1 {
                for dz in -1i32..=1 {
                    biome_positions.insert((entry.x + dx, entry.z + dz));
                }
            }
        }

        let reference_target_positions = if includes_features {
            sorted_positions(&feature_carver_positions)
        } else {
            test_entries
                .iter()
                .map(|entry| (entry.x, entry.z))
                .collect::<Vec<_>>()
        };
        if GENERATE_STRUCTURES {
            for &(target_x, target_z) in &reference_target_positions {
                for dx in -8i32..=8 {
                    for dz in -8i32..=8 {
                        starts_positions.insert((target_x + dx, target_z + dz));
                    }
                }
            }
        }
        if includes_features {
            for &(x, z) in &feature_carver_positions {
                for dx in -1i32..=1 {
                    for dz in -1i32..=1 {
                        biome_positions.insert((x + dx, z + dz));
                    }
                }
            }
        }
        if !GENERATE_STRUCTURES {
            starts_positions.extend(biome_positions.iter().copied());
        }

        let mut chunks: FxHashMap<(i32, i32), ChunkAccess> =
            FxHashMap::with_capacity_and_hasher(starts_positions.len(), FxBuildHasher);
        for &pos in &starts_positions {
            chunks.insert(pos, empty_proto_chunk(pos, section_count, min_y, height));
        }
        eprintln!(
            "[{dim_short}] Allocated {} proto chunks (structures: {GENERATE_STRUCTURES})",
            chunks.len()
        );

        // STRUCTURE_STARTS — per-chunk; uses biome_source directly (no chunk biomes
        // required). Most chunks early-exit at `placement.is_structure_chunk`.
        if GENERATE_STRUCTURES {
            for chunk in chunks.values() {
                generator.create_structures(chunk);
            }
        }

        // BIOMES — only for the 3×3 around each test chunk (surface stage's lookup).
        for &pos in &biome_positions {
            generator.create_biomes(chunk_or_panic(&chunks, pos));
        }

        // STRUCTURE_REFERENCES — mirror of `generate_references`: scan 17×17 for each
        // chunk that will be read at noise/carver stage, recording which neighbor chunks
        // hold a start whose inflated BB intersects it.
        if GENERATE_STRUCTURES {
            for &(target_x, target_z) in &reference_target_positions {
                let target_block_x = target_x * 16;
                let target_block_z = target_z * 16;

                for source_x in (target_x - 8)..=(target_x + 8) {
                    for source_z in (target_z - 8)..=(target_z + 8) {
                        let Some(source_chunk) = chunks.get(&(source_x, source_z)) else {
                            continue;
                        };
                        let starts = source_chunk.structure_starts();
                        for (structure_id, start) in starts.iter() {
                            // `start.bounding_box` is already inflated by `bb_inflate`,
                            // matching `worldgen::stages::structures::generate_references`.
                            let Some(bb) = start.bounding_box else {
                                continue;
                            };
                            if bb.intersects_xz(
                                target_block_x,
                                target_block_z,
                                target_block_x + 15,
                                target_block_z + 15,
                            ) {
                                chunk_or_panic(&chunks, (target_x, target_z))
                                    .structure_references_mut()
                                    .entry(structure_id.clone())
                                    .or_default()
                                    .insert(ChunkPos::new(source_x, source_z));
                            }
                        }
                    }
                }
            }
        }

        // NOISE — fill_from_noise with per-chunk beardifier built from references.
        let noise_positions = if includes_features {
            sorted_positions(&feature_carver_positions)
        } else {
            test_entries
                .iter()
                .map(|entry| (entry.x, entry.z))
                .collect()
        };
        for pos in noise_positions {
            let chunk = chunk_or_panic(&chunks, pos);
            let beardifier = if GENERATE_STRUCTURES {
                build_test_beardifier(chunk, &chunks)
            } else {
                None
            };
            generator.fill_from_noise(chunk, beardifier.as_ref());
        }

        for &stage in STAGES {
            if debug_stage.as_deref().is_some_and(|filter| filter != stage) {
                continue;
            }
            let reference_blocks = load_reference_blocks(stage, dim_short);
            let has_reference = reference_blocks.is_some();

            let stage_entries: Vec<_> = test_entries
                .iter()
                .filter_map(|e| e.stages.get(stage).map(|hash| (e.x, e.z, hash.as_str())))
                .collect();
            let total = stage_entries.len();
            let mut mismatches = Vec::new();
            let feature_holders = if stage == FEATURE_STAGE {
                // Vanilla requests all sampled chunks to CARVERS first, then requests
                // FEATURES in x/z order. Untracked radius-1 dependencies must reach
                // CARVERS, but their feature stage must not run.
                let dependency_positions = sorted_positions(&feature_carver_positions);
                let feature_stage_only = debug_stage.as_deref() == Some(FEATURE_STAGE);
                for &pos in &dependency_positions {
                    if !feature_stage_only && tracked_positions.contains(&pos) {
                        continue;
                    }
                    let chunk = chunk_or_panic(&chunks, pos);
                    let neighbor_biomes = |qx: i32, qy: i32, qz: i32| -> u16 {
                        let cx = qx >> 2;
                        let cz = qz >> 2;
                        let neighbor = chunk_or_panic(&chunks, (cx, cz));
                        let sections = neighbor.sections();
                        let local_qx = (qx - cx * 4) as usize;
                        let local_qz = (qz - cz * 4) as usize;
                        let qy_clamped = (qy - min_qy).clamp(0, total_quarts_y - 1) as usize;
                        let section_idx = qy_clamped / 4;
                        let local_qy = qy_clamped % 4;
                        sections.sections[section_idx]
                            .read()
                            .biomes
                            .get(local_qx, local_qy, local_qz)
                    };
                    generator.build_surface(chunk, &neighbor_biomes);
                }
                for &pos in &dependency_positions {
                    if !feature_stage_only && tracked_positions.contains(&pos) {
                        continue;
                    }
                    generator.apply_carvers(chunk_or_panic(&chunks, pos));
                }
                Some(Arc::new(build_feature_holders(
                    mem::take(&mut chunks),
                    &feature_carver_positions,
                    min_y,
                    height,
                )))
            } else {
                None
            };

            if stage == FEATURE_STAGE {
                let Some(holders) = &feature_holders else {
                    panic!("features stage missing chunk holders");
                };
                let Some(context) = &feature_context else {
                    panic!("features stage missing worldgen context");
                };

                for &(chunk_x, chunk_z, _) in &stage_entries {
                    let center = ChunkPos::new(chunk_x, chunk_z);
                    let Some(center_holder) = holders.get(&(chunk_x, chunk_z)) else {
                        panic!("Missing feature center chunk ({chunk_x}, {chunk_z})");
                    };
                    {
                        let Some(chunk) = center_holder.try_chunk(ChunkStatus::Carvers) else {
                            panic!("Feature center chunk ({chunk_x}, {chunk_z}) missing");
                        };
                        chunk.prime_final_heightmaps();
                    }
                    let cache_holders = holders.clone();
                    let cache = Arc::new(StaticCache2D::create(
                        chunk_x,
                        chunk_z,
                        feature_cache_radius,
                        move |x, z| match cache_holders.get(&(x, z)) {
                            Some(holder) => holder.clone(),
                            None => panic!("Missing feature dependency chunk ({x}, {z})"),
                        },
                    ));
                    let region_random =
                        generator.create_worldgen_region_random(seed as i64, center);
                    let mut region = steel_core::worldgen::WorldGenRegion::new(
                        context,
                        feature_step,
                        &cache,
                        center,
                        region_random,
                    );
                    generator.apply_biome_decorations(&mut region);
                }
            }

            for (i, &(chunk_x, chunk_z, expected_hash)) in stage_entries.iter().enumerate() {
                let actual_hash = if stage == FEATURE_STAGE {
                    let Some(holders) = &feature_holders else {
                        panic!("features stage missing chunk holders");
                    };
                    let Some(holder) = holders.get(&(chunk_x, chunk_z)) else {
                        panic!("Missing feature center chunk ({chunk_x}, {chunk_z})");
                    };
                    let Some(chunk) = holder.try_chunk(ChunkStatus::Carvers) else {
                        panic!("Feature center chunk ({chunk_x}, {chunk_z}) missing");
                    };
                    compute_block_hash(chunk.sections())
                } else {
                    let chunk = chunk_or_panic(&chunks, (chunk_x, chunk_z));

                    // Apply current stage (structure_starts, references, biomes, noise
                    // already done by pre-pass).
                    if stage != "minecraft:noise" {
                        let neighbor_biomes = |qx: i32, qy: i32, qz: i32| -> u16 {
                            let cx = qx >> 2;
                            let cz = qz >> 2;
                            let neighbor = chunk_or_panic(&chunks, (cx, cz));
                            let sections = neighbor.sections();
                            let local_qx = (qx - cx * 4) as usize;
                            let local_qz = (qz - cz * 4) as usize;
                            let qy_clamped = (qy - min_qy).clamp(0, total_quarts_y - 1) as usize;
                            let section_idx = qy_clamped / 4;
                            let local_qy = qy_clamped % 4;
                            sections.sections[section_idx]
                                .read()
                                .biomes
                                .get(local_qx, local_qy, local_qz)
                        };

                        match stage {
                            "minecraft:surface" => generator.build_surface(chunk, &neighbor_biomes),
                            "minecraft:carvers" => generator.apply_carvers(chunk),
                            _ => panic!("Stage {stage} not yet implemented in test harness"),
                        }
                    }

                    compute_block_hash(chunk.sections())
                };

                let ok = actual_hash == expected_hash;
                if (i + 1) % 10 == 0 || i + 1 == total || !ok {
                    let status = if ok { "OK" } else { "MISMATCH" };
                    eprintln!(
                        "[{dim_short}/{stage}] ({chunk_x:3},{chunk_z:3}) {status} expected={expected_hash} actual={actual_hash}  [{}/{total}]",
                        i + 1,
                    );
                }

                if actual_hash != expected_hash {
                    let block_diffs = reference_blocks
                        .as_ref()
                        .and_then(|refs| refs.get(&(chunk_x, chunk_z)))
                        .map(|ref_data| {
                            if stage == FEATURE_STAGE {
                                let Some(holders) = &feature_holders else {
                                    panic!("features stage missing chunk holders");
                                };
                                let Some(holder) = holders.get(&(chunk_x, chunk_z)) else {
                                    panic!("Missing feature center chunk ({chunk_x}, {chunk_z})");
                                };
                                let Some(chunk) = holder.try_chunk(ChunkStatus::Carvers) else {
                                    panic!("Feature center chunk ({chunk_x}, {chunk_z}) missing");
                                };
                                diff_chunk(chunk.sections(), ref_data, min_y)
                            } else {
                                let chunk = chunk_or_panic(&chunks, (chunk_x, chunk_z));
                                diff_chunk(chunk.sections(), ref_data, min_y)
                            }
                        });

                    mismatches.push((
                        chunk_x,
                        chunk_z,
                        expected_hash.to_owned(),
                        actual_hash,
                        block_diffs,
                    ));
                    if stop_after_first_mismatch {
                        break;
                    }
                }
            }

            if mismatches.is_empty() {
                continue;
            }

            let failed = mismatches.len();
            let mut msg =
                format!("{dim_short}/{stage}: {failed}/{total} chunks do not match vanilla");
            if !has_reference {
                msg.push_str(" (no binary reference data, showing hashes only)");
            }
            msg.push('\n');

            for (x, z, expected_hash, actual_hash, block_diffs) in &mismatches {
                match block_diffs {
                    Some(diffs) if !diffs.is_empty() => {
                        msg.push_str(&format_chunk_diffs(diffs, *x, *z, min_y));
                    }
                    _ => {
                        let _ = writeln!(
                            msg,
                            "  ({x:3},{z:3}): expected {expected_hash}, got {actual_hash}"
                        );
                    }
                }
            }

            panic!("{msg}");
        }
    }
}
