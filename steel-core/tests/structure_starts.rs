//! Structure starts regression test.
//!
//! Verifies that Steel's `create_structures` matches vanilla Minecraft for the
//! seed in `test_assets/structure_starts.json`. For each chunk that vanilla
//! recorded as having starts, runs structure generation and compares structure
//! ids, references, bounding boxes, piece types, gen depths, orientations, and
//! piece bounding boxes.
//!
//! The JSON only lists chunks that contain at least one start, so this test
//! validates positive cases only — it cannot directly catch false positives in
//! chunks vanilla left empty. Pair with the chunk-stage hashes test for noise
//! coverage (noise depends on structure starts via Beardifier).
//!
//! `nbt_data`, `ground_level_delta`, `junctions`, and `bb_inflate` are not in
//! the JSON and are not compared.

use std::fmt::Write as _;

use rustc_hash::FxHashMap;
use serde::Deserialize;
use steel_core::world::structure::StructureStart;
use steel_utils::{Direction, Identifier};

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
struct ExpectedBoundingBox {
    min_x: i32,
    min_y: i32,
    min_z: i32,
    max_x: i32,
    max_y: i32,
    max_z: i32,
}

impl ExpectedBoundingBox {
    const fn matches(&self, actual: &steel_utils::BoundingBox) -> bool {
        self.min_x == actual.min_x
            && self.min_y == actual.min_y
            && self.min_z == actual.min_z
            && self.max_x == actual.max_x
            && self.max_y == actual.max_y
            && self.max_z == actual.max_z
    }
}

#[derive(Deserialize, Debug)]
struct ExpectedPiece {
    #[serde(rename = "type")]
    piece_type: String,
    gen_depth: i32,
    orientation: i32,
    bounding_box: ExpectedBoundingBox,
}

#[derive(Deserialize, Debug)]
struct ExpectedStart {
    structure: String,
    chunk_x: i32,
    chunk_z: i32,
    references: i32,
    bounding_box: ExpectedBoundingBox,
    pieces: Vec<ExpectedPiece>,
}

#[derive(Deserialize, Debug)]
struct ExpectedChunk {
    x: i32,
    z: i32,
    starts: Vec<ExpectedStart>,
}

#[derive(Deserialize, Debug)]
struct ExpectedDimension {
    chunks_with_starts: u32,
    total_starts: u32,
    total_pieces: u32,
    chunks: Vec<ExpectedChunk>,
}

#[derive(Deserialize, Debug)]
struct ExpectedJson {
    seed: u64,
    overworld: ExpectedDimension,
    the_nether: ExpectedDimension,
    the_end: ExpectedDimension,
}

fn load_expected() -> ExpectedJson {
    let json = include_str!("../test_assets/structure_starts.json");
    serde_json::from_str(json).expect("Failed to parse structure_starts.json")
}

/// Vanilla's `Direction.get2DDataValue()`:
/// SOUTH = 0, WEST = 1, NORTH = 2, EAST = 3, vertical/null = -1.
const fn direction_to_2d(orientation: Option<Direction>) -> i32 {
    match orientation {
        Some(Direction::South) => 0,
        Some(Direction::West) => 1,
        Some(Direction::North) => 2,
        Some(Direction::East) => 3,
        Some(Direction::Down | Direction::Up) | None => -1,
    }
}

/// Format a steel `BoundingBox` for inclusion in error messages.
fn fmt_bb_actual(bb: &steel_utils::BoundingBox) -> String {
    format!(
        "[{},{},{} .. {},{},{}]",
        bb.min_x, bb.min_y, bb.min_z, bb.max_x, bb.max_y, bb.max_z,
    )
}

fn fmt_bb_expected(bb: &ExpectedBoundingBox) -> String {
    format!(
        "[{},{},{} .. {},{},{}]",
        bb.min_x, bb.min_y, bb.min_z, bb.max_x, bb.max_y, bb.max_z,
    )
}

#[test]
#[ignore = "This test takes too long to run for normal testing"]
fn structure_starts() {
    use std::panic;
    use std::thread;

    // Larger stack to match chunk_stage_hashes.rs — jigsaw assembly recurses
    // deeply for large structure sets like end_city.
    let result = thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(structure_starts_inner)
        .expect("Failed to spawn test thread")
        .join();

    if let Err(payload) = result {
        panic::resume_unwind(payload);
    }
}

const DIMENSION_ORDER: &[&str] = &["overworld", "the_nether", "the_end"];

#[expect(
    clippy::too_many_lines,
    reason = "large test with per-dimension setup and per-chunk assertions"
)]
fn structure_starts_inner() {
    use steel_core::chunk::chunk_access::ChunkAccess;
    use steel_core::chunk::proto_chunk::ProtoChunk;
    use steel_core::chunk::section::{ChunkSection, Sections};
    use steel_core::worldgen::{
        BiomeSourceKind, ChunkGenerator, ChunkGeneratorType, EndGenerator, NetherGenerator,
        OverworldGenerator,
    };
    use steel_registry::{REGISTRY, Registry, vanilla_dimension_types};
    use steel_utils::ChunkPos;

    let mut registry = Registry::new_vanilla();
    registry.freeze();
    let _ = REGISTRY.init(registry);

    let expected = load_expected();
    let seed = expected.seed;
    assert_eq!(seed, 13579, "Expected seed 13579");

    let mut total_failures = 0usize;
    let mut report = String::new();

    for &dim_short in DIMENSION_ORDER {
        let dim_data = match dim_short {
            "overworld" => &expected.overworld,
            "the_nether" => &expected.the_nether,
            "the_end" => &expected.the_end,
            _ => unreachable!(),
        };

        let dim_type = match dim_short {
            "overworld" => &vanilla_dimension_types::OVERWORLD,
            "the_nether" => &vanilla_dimension_types::THE_NETHER,
            "the_end" => &vanilla_dimension_types::THE_END,
            _ => unreachable!(),
        };

        let min_y = dim_type.min_y;
        let height = dim_type.height;
        let section_count = (height / 16) as usize;

        let generator: ChunkGeneratorType = match dim_short {
            "overworld" => {
                let source = BiomeSourceKind::overworld(seed);
                ChunkGeneratorType::Overworld(OverworldGenerator::new(source, seed))
            }
            "the_nether" => {
                let source = BiomeSourceKind::nether(seed);
                ChunkGeneratorType::Nether(NetherGenerator::new(source, seed))
            }
            "the_end" => {
                let source = BiomeSourceKind::end(seed);
                ChunkGeneratorType::End(EndGenerator::new(source, seed))
            }
            _ => unreachable!(),
        };

        eprintln!(
            "=== {dim_short} ({} chunks, {} starts, {} pieces) ===",
            dim_data.chunks_with_starts, dim_data.total_starts, dim_data.total_pieces,
        );

        let mut chunks_sorted: Vec<&ExpectedChunk> = dim_data.chunks.iter().collect();
        chunks_sorted.sort_by_key(|c| (c.x, c.z));

        let total = chunks_sorted.len();
        let mut dim_failures = 0usize;
        let mut dim_report = String::new();

        for (i, chunk_data) in chunks_sorted.iter().enumerate() {
            let sections: Box<[ChunkSection]> = (0..section_count)
                .map(|_| ChunkSection::new_empty())
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let proto = ProtoChunk::new(
                Sections::from_owned(sections),
                ChunkPos::new(chunk_data.x, chunk_data.z),
                min_y,
                height,
            );
            let chunk = ChunkAccess::Proto(proto);

            generator.create_structures(&chunk);

            let actual_starts = chunk.structure_starts();
            let chunk_errors = compare_chunk(chunk_data, &actual_starts);

            if (i + 1) % 25 == 0 || i + 1 == total || !chunk_errors.is_empty() {
                let status = if chunk_errors.is_empty() {
                    "OK"
                } else {
                    "FAIL"
                };
                eprintln!(
                    "[{dim_short}] ({:4},{:4}) {status}  [{}/{total}]",
                    chunk_data.x,
                    chunk_data.z,
                    i + 1,
                );
            }

            if !chunk_errors.is_empty() {
                dim_failures += 1;
                let _ = writeln!(dim_report, "  Chunk ({}, {}):", chunk_data.x, chunk_data.z);
                for err in &chunk_errors {
                    for line in err.lines() {
                        let _ = writeln!(dim_report, "    {line}");
                    }
                }
            }
        }

        if dim_failures > 0 {
            total_failures += dim_failures;
            let _ = writeln!(
                report,
                "{dim_short}: {dim_failures}/{total} chunks do not match vanilla",
            );
            report.push_str(&dim_report);
        }
    }

    assert!(total_failures == 0, "structure starts mismatch:\n{report}");
}

/// Compare the actual structure-start map for a chunk against the JSON
/// expectations. Returns one human-readable error string per mismatched
/// structure.
fn compare_chunk(
    expected: &ExpectedChunk,
    actual: &FxHashMap<Identifier, StructureStart>,
) -> Vec<String> {
    let mut errors = Vec::new();

    let mut expected_by_id: FxHashMap<&str, &ExpectedStart> = FxHashMap::default();
    for start in &expected.starts {
        expected_by_id.insert(start.structure.as_str(), start);
    }

    let mut actual_by_id: FxHashMap<String, &StructureStart> = FxHashMap::default();
    for (id, start) in actual {
        actual_by_id.insert(format!("{id}"), start);
    }

    let mut expected_keys: Vec<&str> = expected_by_id.keys().copied().collect();
    expected_keys.sort_unstable();

    for key in &expected_keys {
        let exp = expected_by_id[key];
        let Some(actual_start) = actual_by_id.get(*key) else {
            errors.push(format!(
                "missing start `{key}`: expected {} pieces, bb {}",
                exp.pieces.len(),
                fmt_bb_expected(&exp.bounding_box),
            ));
            continue;
        };

        if let Some(err) = compare_start(exp, actual_start) {
            errors.push(err);
        }
    }

    let mut actual_keys: Vec<&String> = actual_by_id.keys().collect();
    actual_keys.sort();
    for key in &actual_keys {
        if !expected_by_id.contains_key(key.as_str()) {
            errors.push(format!("unexpected start `{key}` not in JSON"));
        }
    }

    errors
}

fn compare_start(expected: &ExpectedStart, actual: &StructureStart) -> Option<String> {
    let mut diffs = Vec::new();

    let actual_chunk = actual.chunk_pos;
    if expected.chunk_x != actual_chunk.0.x || expected.chunk_z != actual_chunk.0.y {
        diffs.push(format!(
            "chunk_pos: expected ({}, {}), got ({}, {})",
            expected.chunk_x, expected.chunk_z, actual_chunk.0.x, actual_chunk.0.y,
        ));
    }

    if expected.references != actual.references {
        diffs.push(format!(
            "references: expected {}, got {}",
            expected.references, actual.references,
        ));
    }

    match actual.bounding_box {
        Some(actual_bb) if expected.bounding_box.matches(&actual_bb) => {}
        Some(actual_bb) => diffs.push(format!(
            "bounding_box: expected {}, got {}",
            fmt_bb_expected(&expected.bounding_box),
            fmt_bb_actual(&actual_bb),
        )),
        None => diffs.push(format!(
            "bounding_box: expected {}, got None",
            fmt_bb_expected(&expected.bounding_box),
        )),
    }

    if expected.pieces.len() != actual.pieces.len() {
        diffs.push(format!(
            "piece count: expected {}, got {}",
            expected.pieces.len(),
            actual.pieces.len(),
        ));
    }

    let common = expected.pieces.len().min(actual.pieces.len());
    for i in 0..common {
        let exp_piece = &expected.pieces[i];
        let act_piece = &actual.pieces[i];

        let actual_type = format!("{}", act_piece.piece_type);
        if exp_piece.piece_type != actual_type {
            diffs.push(format!(
                "piece[{i}].type: expected `{}`, got `{}`",
                exp_piece.piece_type, actual_type,
            ));
        }

        if exp_piece.gen_depth != act_piece.gen_depth {
            diffs.push(format!(
                "piece[{i}].gen_depth: expected {}, got {}",
                exp_piece.gen_depth, act_piece.gen_depth,
            ));
        }

        let actual_orient = direction_to_2d(act_piece.orientation);
        if exp_piece.orientation != actual_orient {
            diffs.push(format!(
                "piece[{i}].orientation: expected {} ({}), got {} ({:?})",
                exp_piece.orientation,
                orient_name(exp_piece.orientation),
                actual_orient,
                act_piece.orientation,
            ));
        }

        if !exp_piece.bounding_box.matches(&act_piece.bounding_box) {
            diffs.push(format!(
                "piece[{i}].bb: expected {}, got {}",
                fmt_bb_expected(&exp_piece.bounding_box),
                fmt_bb_actual(&act_piece.bounding_box),
            ));
        }
    }

    if diffs.is_empty() {
        return None;
    }

    let mut msg = format!("start `{}`:\n", expected.structure);
    let total = diffs.len();
    let shown = total.min(MAX_DIFFS_PER_START);
    for d in diffs.iter().take(shown) {
        let _ = writeln!(msg, "  {d}");
    }
    if total > shown {
        let _ = writeln!(msg, "  ... and {} more diffs", total - shown);
    }
    Some(msg.trim_end().to_owned())
}

/// Maximum diffs shown per `StructureStart` before truncating. Matches the
/// per-chunk cap in `chunk_stage_hashes.rs` — keeps multi-piece structures like
/// `end_city` from drowning the report.
const MAX_DIFFS_PER_START: usize = 30;

const fn orient_name(data2d: i32) -> &'static str {
    match data2d {
        0 => "south",
        1 => "west",
        2 => "north",
        3 => "east",
        _ => "none",
    }
}
