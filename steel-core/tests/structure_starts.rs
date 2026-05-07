//! Structure starts regression test.
//!
//! Verifies that Steel's `create_structures` matches vanilla Minecraft for the
//! seed in `test_assets/structure_starts.json`. For each chunk that vanilla
//! recorded as having starts, runs structure generation and compares structure
//! ids, references, structure-reference maps, bounding boxes, piece types, gen
//! depths, orientations, piece bounding boxes, and per-piece NBT.
//!
//! The JSON lists chunks with starts or references. It still does not validate
//! completely empty chunks, so pair with the chunk-stage hashes test for noise
//! coverage (noise depends on structure starts via Beardifier).

use std::fmt::Write as _;
use std::io::Cursor;
use std::mem::take;

use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use simdnbt::owned::read_compound;
use steel_core::chunk::chunk_access::ChunkAccess;
use steel_core::chunk::proto_chunk::ProtoChunk;
use steel_core::chunk::section::{ChunkSection, Sections};
use steel_core::world::structure::{
    StructurePiece, StructureReferenceMap, StructureStart, StructureStartMap,
};
use steel_registry::template_pool::Projection;
use steel_utils::{ChunkPos, Direction, Identifier};

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
    #[serde(default)]
    nbt: Option<ExpectedPieceNbt>,
}

#[derive(Deserialize, Serialize, Debug)]
struct ExpectedPieceNbt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ground_level_delta: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    junctions: Vec<ExpectedJunction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pool_element: Option<ExpectedPoolElement>,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq)]
struct ExpectedJunction {
    source_x: i32,
    source_ground_y: i32,
    source_z: i32,
    delta_y: i32,
    dest_proj: String,
}

#[derive(Deserialize, Serialize, Debug)]
struct ExpectedPoolElement {
    projection: String,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
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
    #[serde(default)]
    references: Vec<ExpectedReference>,
}

#[derive(Deserialize, Debug)]
struct ExpectedReference {
    structure: String,
    source_chunks: Vec<[i32; 2]>,
}

#[derive(Deserialize, Debug)]
struct ExpectedDimension {
    chunks_with_starts: u32,
    chunks_with_references: u32,
    total_starts: u32,
    total_pieces: u32,
    total_references: u32,
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

fn make_proto_chunk(pos: (i32, i32), section_count: usize, min_y: i32, height: i32) -> ChunkAccess {
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
    ChunkAccess::Proto(proto)
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
    use steel_core::worldgen::{
        BiomeSourceKind, ChunkGenerator, ChunkGeneratorType, EndGenerator, NetherGenerator,
        OverworldGenerator,
    };
    use steel_registry::{REGISTRY, Registry, vanilla_dimension_types};

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
            "=== {dim_short} ({} start chunks, {} reference chunks, {} starts, {} pieces, {} references) ===",
            dim_data.chunks_with_starts,
            dim_data.chunks_with_references,
            dim_data.total_starts,
            dim_data.total_pieces,
            dim_data.total_references,
        );

        let mut chunks_sorted: Vec<&ExpectedChunk> = dim_data.chunks.iter().collect();
        chunks_sorted.sort_by_key(|c| (c.x, c.z));

        let mut source_positions = FxHashSet::default();
        for chunk_data in &chunks_sorted {
            for dx in -8i32..=8 {
                for dz in -8i32..=8 {
                    source_positions.insert((chunk_data.x + dx, chunk_data.z + dz));
                }
            }
        }

        let mut actual_starts_by_pos: FxHashMap<(i32, i32), StructureStartMap> =
            FxHashMap::default();
        for pos in source_positions {
            let chunk = make_proto_chunk(pos, section_count, min_y, height);
            generator.create_structures(&chunk);

            let mut starts = chunk.structure_starts_mut();
            if !starts.is_empty() {
                actual_starts_by_pos.insert(pos, take(&mut *starts));
            }
        }

        eprintln!(
            "[{dim_short}] Generated non-empty starts for {} chunks",
            actual_starts_by_pos.len(),
        );

        let total = chunks_sorted.len();
        let mut dim_failures = 0usize;
        let mut dim_report = String::new();

        for (i, chunk_data) in chunks_sorted.iter().enumerate() {
            let actual_references =
                collect_references_for_chunk(chunk_data.x, chunk_data.z, &actual_starts_by_pos);
            let empty_starts = StructureStartMap::default();
            let actual_starts = actual_starts_by_pos
                .get(&(chunk_data.x, chunk_data.z))
                .unwrap_or(&empty_starts);
            let chunk_errors = compare_chunk(chunk_data, actual_starts, &actual_references);

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

fn collect_references_for_chunk(
    target_x: i32,
    target_z: i32,
    starts_by_pos: &FxHashMap<(i32, i32), StructureStartMap>,
) -> StructureReferenceMap {
    let mut references = StructureReferenceMap::default();
    let target_block_x = target_x * 16;
    let target_block_z = target_z * 16;

    for source_x in (target_x - 8)..=(target_x + 8) {
        for source_z in (target_z - 8)..=(target_z + 8) {
            let Some(starts) = starts_by_pos.get(&(source_x, source_z)) else {
                continue;
            };

            for (structure_id, start) in starts {
                let Some(bb) = start.bounding_box else {
                    continue;
                };
                if bb.intersects_xz(
                    target_block_x,
                    target_block_z,
                    target_block_x + 15,
                    target_block_z + 15,
                ) {
                    references
                        .entry(structure_id.clone())
                        .or_default()
                        .push(ChunkPos::new(source_x, source_z));
                }
            }
        }
    }

    references
}

/// Compare the actual structure maps for a chunk against the JSON expectations.
/// Returns one human-readable error string per mismatched structure/reference.
fn compare_chunk(
    expected: &ExpectedChunk,
    actual_starts: &FxHashMap<Identifier, StructureStart>,
    actual_references: &StructureReferenceMap,
) -> Vec<String> {
    let mut errors = Vec::new();

    let mut expected_by_id: FxHashMap<&str, &ExpectedStart> = FxHashMap::default();
    for start in &expected.starts {
        expected_by_id.insert(start.structure.as_str(), start);
    }

    let mut actual_by_id: FxHashMap<String, &StructureStart> = FxHashMap::default();
    for (id, start) in actual_starts {
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

    errors.extend(compare_references(&expected.references, actual_references));

    errors
}

fn compare_references(
    expected: &[ExpectedReference],
    actual: &StructureReferenceMap,
) -> Vec<String> {
    let mut errors = Vec::new();

    let mut expected_by_id: FxHashMap<&str, Vec<(i32, i32)>> = FxHashMap::default();
    for reference in expected {
        let mut source_chunks: Vec<(i32, i32)> = reference
            .source_chunks
            .iter()
            .map(|chunk| (chunk[0], chunk[1]))
            .collect();
        source_chunks.sort_unstable();
        expected_by_id.insert(reference.structure.as_str(), source_chunks);
    }

    let mut actual_by_id: FxHashMap<String, Vec<(i32, i32)>> = FxHashMap::default();
    for (id, source_chunks) in actual {
        let mut sorted: Vec<(i32, i32)> = source_chunks
            .iter()
            .map(|chunk| (chunk.0.x, chunk.0.y))
            .collect();
        sorted.sort_unstable();
        actual_by_id.insert(format!("{id}"), sorted);
    }

    let mut expected_keys: Vec<&str> = expected_by_id.keys().copied().collect();
    expected_keys.sort_unstable();
    for key in expected_keys {
        let expected_sources = &expected_by_id[key];
        let Some(actual_sources) = actual_by_id.get(key) else {
            errors.push(format!(
                "missing references `{key}`: expected {} source chunks",
                expected_sources.len(),
            ));
            continue;
        };

        if expected_sources != actual_sources {
            errors.push(format!(
                "references `{key}`: expected {}, got {}",
                fmt_chunk_list(expected_sources),
                fmt_chunk_list(actual_sources),
            ));
        }
    }

    let mut actual_keys: Vec<&String> = actual_by_id.keys().collect();
    actual_keys.sort();
    for key in actual_keys {
        if !expected_by_id.contains_key(key.as_str()) {
            errors.push(format!(
                "unexpected references `{key}`: got {}",
                fmt_chunk_list(&actual_by_id[key]),
            ));
        }
    }

    errors
}

fn fmt_chunk_list(chunks: &[(i32, i32)]) -> String {
    const MAX_CHUNKS_SHOWN: usize = 8;

    let mut out = String::from("[");
    for (i, (x, z)) in chunks.iter().take(MAX_CHUNKS_SHOWN).enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let _ = write!(out, "({x},{z})");
    }
    if chunks.len() > MAX_CHUNKS_SHOWN {
        let _ = write!(out, ", ... +{} more", chunks.len() - MAX_CHUNKS_SHOWN);
    }
    out.push(']');
    out
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

        compare_piece_nbt(i, exp_piece.nbt.as_ref(), act_piece, &mut diffs);
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

fn compare_piece_nbt(
    index: usize,
    expected: Option<&ExpectedPieceNbt>,
    actual: &StructurePiece,
    diffs: &mut Vec<String>,
) {
    let Some(expected_nbt) = expected else {
        if !actual.nbt_data.is_empty() {
            diffs.push(format!(
                "piece[{index}].nbt: expected no NBT, got {} bytes",
                actual.nbt_data.len(),
            ));
        }
        return;
    };

    if let Some(expected_delta) = expected_nbt.ground_level_delta
        && expected_delta != actual.ground_level_delta
    {
        diffs.push(format!(
            "piece[{index}].nbt.ground_level_delta: expected {}, got {}",
            expected_delta, actual.ground_level_delta,
        ));
    }

    if let Some(pool_element) = &expected_nbt.pool_element {
        let actual_projection = projection_to_name(actual.projection);
        if Some(pool_element.projection.as_str()) != actual_projection {
            diffs.push(format!(
                "piece[{index}].nbt.pool_element.projection: expected `{}`, got {}",
                pool_element.projection,
                actual_projection.unwrap_or("none"),
            ));
        }
    }

    compare_junctions(index, &expected_nbt.junctions, actual, diffs);
    compare_raw_nbt(index, expected_nbt, &actual.nbt_data, diffs);
}

fn compare_junctions(
    index: usize,
    expected: &[ExpectedJunction],
    actual: &StructurePiece,
    diffs: &mut Vec<String>,
) {
    if expected.len() != actual.junctions.len() {
        diffs.push(format!(
            "piece[{index}].nbt.junction count: expected {}, got {}",
            expected.len(),
            actual.junctions.len(),
        ));
    }

    let common = expected.len().min(actual.junctions.len());
    for (junction_index, expected_junction) in expected.iter().take(common).enumerate() {
        let actual_junction = &actual.junctions[junction_index];
        let actual_dest_projection = projection_to_name(Some(actual_junction.dest_projection));

        if expected_junction.source_x != actual_junction.source_x {
            diffs.push(format!(
                "piece[{index}].nbt.junctions[{junction_index}].source_x: expected {}, got {}",
                expected_junction.source_x, actual_junction.source_x,
            ));
        }
        if expected_junction.source_ground_y != actual_junction.source_ground_y {
            diffs.push(format!(
                "piece[{index}].nbt.junctions[{junction_index}].source_ground_y: expected {}, got {}",
                expected_junction.source_ground_y, actual_junction.source_ground_y,
            ));
        }
        if expected_junction.source_z != actual_junction.source_z {
            diffs.push(format!(
                "piece[{index}].nbt.junctions[{junction_index}].source_z: expected {}, got {}",
                expected_junction.source_z, actual_junction.source_z,
            ));
        }
        if expected_junction.delta_y != actual_junction.delta_y {
            diffs.push(format!(
                "piece[{index}].nbt.junctions[{junction_index}].delta_y: expected {}, got {}",
                expected_junction.delta_y, actual_junction.delta_y,
            ));
        }
        if Some(expected_junction.dest_proj.as_str()) != actual_dest_projection {
            diffs.push(format!(
                "piece[{index}].nbt.junctions[{junction_index}].dest_proj: expected `{}`, got {}",
                expected_junction.dest_proj,
                actual_dest_projection.unwrap_or("none"),
            ));
        }
    }
}

fn compare_raw_nbt(
    index: usize,
    expected_nbt: &ExpectedPieceNbt,
    actual_bytes: &[u8],
    diffs: &mut Vec<String>,
) {
    let expected_value = match serde_json::to_value(expected_nbt) {
        Ok(value) => value,
        Err(err) => {
            diffs.push(format!(
                "piece[{index}].nbt: fixture NBT could not be converted to JSON: {err}",
            ));
            return;
        }
    };

    if actual_bytes.is_empty() {
        if expected_value != Value::Object(serde_json::Map::new()) {
            diffs.push(format!(
                "piece[{index}].nbt: expected {}, got empty nbt_data",
                fmt_json_short(&expected_value),
            ));
        }
        return;
    }

    let actual_value = match actual_nbt_to_json(actual_bytes) {
        Ok(value) => value,
        Err(err) => {
            diffs.push(format!(
                "piece[{index}].nbt: failed to parse actual nbt_data: {err}",
            ));
            return;
        }
    };

    if expected_value != actual_value {
        diffs.push(format!(
            "piece[{index}].nbt: expected {}, got {}",
            fmt_json_short(&expected_value),
            fmt_json_short(&actual_value),
        ));
    }
}

fn actual_nbt_to_json(bytes: &[u8]) -> Result<Value, String> {
    let nbt = read_compound(&mut Cursor::new(bytes)).map_err(|err| format!("{err:?}"))?;
    serde_json::to_value(nbt).map_err(|err| err.to_string())
}

fn fmt_json_short(value: &Value) -> String {
    const MAX_CHARS: usize = 240;

    let text = match serde_json::to_string(value) {
        Ok(text) => text,
        Err(err) => return format!("<failed to format JSON: {err}>"),
    };
    let mut truncated: String = text.chars().take(MAX_CHARS).collect();
    if text.chars().count() > MAX_CHARS {
        truncated.push_str("...");
    }
    truncated
}

const fn projection_to_name(projection: Option<Projection>) -> Option<&'static str> {
    match projection {
        Some(Projection::Rigid) => Some("rigid"),
        Some(Projection::TerrainMatching) => Some("terrain_matching"),
        None => None,
    }
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
