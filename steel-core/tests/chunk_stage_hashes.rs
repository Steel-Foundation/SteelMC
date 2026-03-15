//! Chunk generation stage regression test.
//!
//! Verifies that Steel's chunk generation matches vanilla Minecraft at each stage
//! by comparing MD5 hashes of block data. When a mismatch is found and binary
//! reference data is available, shows exact block-level diffs.
//!
//! Tests all dimensions (overworld, nether, end) using the new JSON format
//! with a `dimensions` wrapper.

use std::fmt::Write;
use std::fs;
use std::io::{BufReader, Cursor, Read as IoRead};

use flate2::read::GzDecoder;
use rustc_hash::{FxBuildHasher, FxHashMap};
use serde::Deserialize;
use steel_core::chunk::section::Sections;

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
    dimensions: FxHashMap<String, DimensionData>,
}

/// Stages to verify. Uncomment as each stage is implemented.
const STAGES: &[&str] = &[
    "minecraft:noise",
    "minecraft:surface",
    // "minecraft:carvers",
    // "minecraft:features",
];

/// Max block-level diffs to show per chunk before truncating.
const MAX_DIFFS_PER_CHUNK: usize = 30;

/// Set specific chunk coordinates to test only those chunks.
/// When non-empty, only these chunks are generated and checked (ignores the JSON list).
/// Example: &[(24, 35)] to debug a single failing chunk.
const DEBUG_CHUNKS: &[(i32, i32)] = &[];

fn load_expected_hashes() -> ChunkStageHashesJson {
    let json_str = include_str!("../test_assets/chunk_stage_hashes.json");
    serde_json::from_str(json_str).expect("Failed to parse chunk_stage_hashes.json")
}

fn compute_block_hash(sections: &Sections) -> String {
    let mut ctx = md5::Context::new();

    for section_holder in &sections.sections {
        let section = section_holder.read();
        if section.states.has_only_air() {
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
#[ignore = "This test takes too long to run for normal testing"]
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

#[allow(clippy::too_many_lines, clippy::similar_names)]
fn chunk_stage_hashes_inner() {
    use steel_core::chunk::chunk_access::ChunkAccess;
    use steel_core::chunk::chunk_generator::ChunkGenerator;
    use steel_core::chunk::proto_chunk::ProtoChunk;
    use steel_core::chunk::section::ChunkSection;
    use steel_core::chunk::world_gen_context::{
        ChunkGeneratorType, EndGenerator, NetherGenerator, OverworldGenerator,
    };
    use steel_core::worldgen::BiomeSourceKind;
    use steel_registry::{REGISTRY, Registry, vanilla_dimension_types};
    use steel_utils::ChunkPos;

    let mut registry = Registry::new_vanilla();
    registry.freeze();
    let _ = REGISTRY.init(registry);

    let expected = load_expected_hashes();
    let seed = expected.seed;
    assert_eq!(seed, 13579, "Expected seed 13579");

    for &dim_key in DIMENSION_ORDER {
        let Some(dim_data) = expected.dimensions.get(dim_key) else {
            continue;
        };

        let dim_short = dim_key.strip_prefix("minecraft:").unwrap_or(dim_key);
        let dim_type = match dim_key {
            "minecraft:overworld" => vanilla_dimension_types::OVERWORLD,
            "minecraft:the_nether" => vanilla_dimension_types::THE_NETHER,
            "minecraft:the_end" => vanilla_dimension_types::THE_END,
            _ => panic!("Unknown dimension: {dim_key}"),
        };

        let min_y = dim_type.min_y;
        let height = dim_type.height;
        let section_count = (height / 16) as usize;
        let min_qy = min_y >> 2;
        let total_quarts_y = (section_count * 4) as i32;

        let generator: ChunkGeneratorType = match dim_key {
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
        };

        eprintln!("=== {dim_key} ===");

        // Filter entries by DEBUG_CHUNKS if set
        let test_entries: Vec<&ChunkStageEntry> = if DEBUG_CHUNKS.is_empty() {
            dim_data.chunks.iter().collect()
        } else {
            dim_data
                .chunks
                .iter()
                .filter(|c| DEBUG_CHUNKS.contains(&(c.x, c.z)))
                .collect()
        };

        // Reuse chunks across stages — each stage builds on the previous
        let mut chunks: FxHashMap<(i32, i32), ChunkAccess> =
            FxHashMap::with_capacity_and_hasher(test_entries.len(), FxBuildHasher);
        // Biome-only neighbors for surface and later stages (positions not in `chunks`)
        let mut biome_neighbors: FxHashMap<(i32, i32), ChunkAccess> = FxHashMap::default();
        let mut neighbors_built = false;

        for &stage in STAGES {
            let reference_blocks = load_reference_blocks(stage, dim_short);
            let has_reference = reference_blocks.is_some();
            let needs_neighbors = stage != "minecraft:noise";

            // Generate biome-only neighbor chunks once before the first post-noise stage.
            // Only creates chunks for 3x3 neighborhood positions not already in the test set.
            if needs_neighbors && !neighbors_built {
                for entry in &test_entries {
                    for dx in -1i32..=1 {
                        for dz in -1i32..=1 {
                            let pos = (entry.x + dx, entry.z + dz);
                            if chunks.contains_key(&pos) || biome_neighbors.contains_key(&pos) {
                                continue;
                            }
                            let sections: Box<[ChunkSection]> = (0..section_count)
                                .map(|_| ChunkSection::new_empty())
                                .collect::<Vec<_>>()
                                .into_boxed_slice();
                            let proto = ProtoChunk::new(
                                Sections::from_owned(sections),
                                ChunkPos::new(pos.0, pos.1),
                                min_y,
                                height,
                            );
                            let chunk = ChunkAccess::Proto(proto);
                            generator.create_biomes(&chunk);
                            biome_neighbors.insert(pos, chunk);
                        }
                    }
                }
                eprintln!(
                    "[{dim_short}] Generated {} biome-only neighbor chunks",
                    biome_neighbors.len()
                );
                neighbors_built = true;
            }

            let stage_entries: Vec<_> = test_entries
                .iter()
                .filter_map(|e| e.stages.get(stage).map(|hash| (e.x, e.z, hash.as_str())))
                .collect();
            let total = stage_entries.len();
            let mut mismatches = Vec::new();

            for (i, &(chunk_x, chunk_z, expected_hash)) in stage_entries.iter().enumerate() {
                // Ensure chunk exists with biomes + noise applied
                chunks.entry((chunk_x, chunk_z)).or_insert_with(|| {
                    let sections: Box<[ChunkSection]> = (0..section_count)
                        .map(|_| ChunkSection::new_empty())
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    let proto = ProtoChunk::new(
                        Sections::from_owned(sections),
                        ChunkPos::new(chunk_x, chunk_z),
                        min_y,
                        height,
                    );
                    let chunk = ChunkAccess::Proto(proto);
                    generator.create_biomes(&chunk);
                    generator.fill_from_noise(&chunk);
                    chunk
                });

                let chunk = &chunks[&(chunk_x, chunk_z)];

                // Apply current stage (noise already applied during chunk creation)
                if stage != "minecraft:noise" {
                    let neighbor_biomes = |qx: i32, qy: i32, qz: i32| -> u16 {
                        let cx = qx >> 2;
                        let cz = qz >> 2;
                        let neighbor = chunks
                            .get(&(cx, cz))
                            .or_else(|| biome_neighbors.get(&(cx, cz)))
                            .unwrap_or_else(|| {
                                panic!("Missing neighbor biome data for chunk ({cx}, {cz})")
                            });
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
                        _ => panic!("Stage {stage} not yet implemented in test harness"),
                    }
                }

                let actual_hash = compute_block_hash(chunk.sections());

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
                        .map(|ref_data| diff_chunk(chunk.sections(), ref_data, min_y));

                    mismatches.push((
                        chunk_x,
                        chunk_z,
                        expected_hash.to_owned(),
                        actual_hash,
                        block_diffs,
                    ));
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
