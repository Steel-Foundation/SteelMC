//! Chunk generation stage regression test.
//!
//! Verifies that Steel's chunk generation matches vanilla Minecraft at each stage
//! by comparing MD5 hashes of block data. When a mismatch is found and binary
//! reference data is available, shows exact block-level diffs.
//!
//! Enable stages one at a time as they are implemented.

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
struct ChunkStageHashesJson {
    seed: u64,
    chunks: Vec<ChunkStageEntry>,
    #[allow(dead_code)]
    chunk_count: usize,
}

/// Stages to verify. Uncomment as each stage is implemented.
const STAGES: &[&str] = &[
    "minecraft:noise",
    // "minecraft:surface",
    // "minecraft:carvers",
    // "minecraft:features",
];

/// Max block-level diffs to show per chunk before truncating.
const MAX_DIFFS_PER_CHUNK: usize = 30;

/// Set specific chunk coordinates to test only those chunks.
/// When non-empty, only these chunks are generated and checked (ignores the JSON list).
/// Example: &[(24, 35)] to debug a single failing chunk.
const DEBUG_CHUNKS: &[(i32, i32)] = &[
    //(24, 35),
    //
];

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
    /// 24 sections, each None (all air) or Some(4096 state IDs in YZX order).
    sections: Vec<Option<Vec<i32>>>,
}

/// Loads binary reference block data for a given stage.
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
fn load_reference_blocks(stage: &str) -> Option<FxHashMap<(i32, i32), ChunkBlockData>> {
    let short_name = stage.strip_prefix("minecraft:").unwrap_or(stage);
    let path = format!(
        "{}/test_assets/chunk_stage_{short_name}_blocks.bin.gz",
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
    use steel_core::chunk::chunk_access::ChunkAccess;
    use steel_core::chunk::chunk_generator::ChunkGenerator;
    use steel_core::chunk::proto_chunk::ProtoChunk;
    use steel_core::chunk::section::ChunkSection;
    use steel_core::chunk::world_gen_context::OverworldGenerator;
    use steel_core::worldgen::BiomeSourceKind;
    use steel_registry::{REGISTRY, Registry};
    use steel_utils::ChunkPos;

    let mut registry = Registry::new_vanilla();
    registry.freeze();
    let _ = REGISTRY.init(registry);

    let expected = load_expected_hashes();
    let seed = expected.seed;
    assert_eq!(seed, 13579, "Expected seed 13579");

    let source = BiomeSourceKind::overworld(seed);
    let generator = OverworldGenerator::new(source, seed);

    let section_count = 24;
    let min_y = -64;
    let height = 384;

    for &stage in STAGES {
        let reference_blocks = load_reference_blocks(stage);
        let has_reference = reference_blocks.is_some();

        let stage_chunks: Vec<_> = if DEBUG_CHUNKS.is_empty() {
            expected
                .chunks
                .iter()
                .filter_map(|c| c.stages.get(stage).map(|hash| (c.x, c.z, hash.clone())))
                .collect()
        } else {
            expected
                .chunks
                .iter()
                .filter(|c| DEBUG_CHUNKS.contains(&(c.x, c.z)))
                .filter_map(|c| c.stages.get(stage).map(|hash| (c.x, c.z, hash.clone())))
                .collect()
        };

        let mut mismatches = Vec::new();

        for (chunk_x, chunk_z, expected_hash) in &stage_chunks {
            let sections: Box<[ChunkSection]> = (0..section_count)
                .map(|_| ChunkSection::new_empty())
                .collect::<Vec<_>>()
                .into_boxed_slice();

            let proto = ProtoChunk::new(
                Sections::from_owned(sections),
                ChunkPos::new(*chunk_x, *chunk_z),
                min_y,
                height,
            );

            let chunk = ChunkAccess::Proto(proto);
            generator.fill_from_noise(&chunk);

            let actual_hash = compute_block_hash(chunk.sections());

            println!(
                "chunk {chunk_x}, {chunk_z}: expected {expected_hash:?}, actual {actual_hash:?}"
            );

            if actual_hash != *expected_hash {
                let block_diffs = reference_blocks
                    .as_ref()
                    .and_then(|refs| refs.get(&(*chunk_x, *chunk_z)))
                    .map(|ref_data| diff_chunk(chunk.sections(), ref_data, min_y));

                mismatches.push((
                    *chunk_x,
                    *chunk_z,
                    expected_hash.clone(),
                    actual_hash,
                    block_diffs,
                ));
            }
        }

        if mismatches.is_empty() {
            continue;
        }

        let total = stage_chunks.len();
        let failed = mismatches.len();
        let mut msg = format!("{stage}: {failed}/{total} chunks do not match vanilla");
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
