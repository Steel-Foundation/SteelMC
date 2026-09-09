//! Large-scale overworld terrain regression test.
//!
//! Checks NOISE and SURFACE block hashes for a large contiguous square of
//! overworld chunks against fixtures extracted from vanilla, so terrain bugs
//! that only show up in rare biomes or at extreme coordinates are caught.
//!
//! This complements [`super::chunk_stage_hashes`], which covers the whole
//! pipeline (carvers, features, light) including structure terrain adaptation,
//! but only over a small sample. This test covers three orders of magnitude
//! more chunks at the two stages that are cheap enough to check at that scale.
//!
//! # Fixture
//!
//! `test_assets/large_scale_overworld_{noise,surface}.csv.gz`, rows of
//! `chunk_x,chunk_z,md5` sorted by x then z, produced by `SteelExtractor`'s
//! `LargeScaleChunkHashes` on seed 13579.
//!
//! That extractor runs BIOMES for each chunk and its ring, then NOISE and
//! SURFACE for the chunk itself. It deliberately skips `STRUCTURE_STARTS` and
//! `STRUCTURE_REFERENCES`, so vanilla's `Beardifier` is always empty and terrain
//! is *not* adapted around structures — hence no beardifier here either.
//! Structure terrain adaptation is covered by `chunk_stage_hashes`.

use std::env;
use std::fmt::Write;
use std::fs;
use std::io::{BufRead, BufReader, Cursor};
use std::sync::Arc;

use crate::chunk::Chunk;
use crate::worldgen::generator::{GenerationChunk, NoisePhase, SurfacePhase};
use crate::worldgen::{ChunkGenerator, ChunkGeneratorType};
use flate2::read::GzDecoder;
use glam::IVec3;
use rustc_hash::{FxBuildHasher, FxHashMap};
use steel_registry::vanilla_dimension_types;
use steel_worldgen::biomes::BiomeSourceKind;

use super::chunk_stage_hashes::{compute_block_hash, empty_proto_chunk};

/// Seed the fixture was extracted with.
const SEED: u64 = 13579;

/// Chunks generated per tile side. A tile plus its one-chunk biome ring is held
/// in memory at once, so this trades memory for less repeated biome work.
const TILE_SIZE: i32 = 32;

/// Shrink the tested area to `[-n, n)` on both axes, for a quicker run.
const HALF_SIZE_ENV: &str = "STEEL_LARGE_SCALE_HALF_SIZE";
/// Stop after this many mismatches instead of running the whole square.
const MAX_MISMATCHES_ENV: &str = "STEEL_LARGE_SCALE_MAX_MISMATCHES";

const DEFAULT_MAX_MISMATCHES: usize = 20;

type HashMapByPos = FxHashMap<(i32, i32), [u8; 16]>;

/// One stage's fixture plus the square it covers.
struct Fixture {
    noise: HashMapByPos,
    surface: HashMapByPos,
    half_size: i32,
}

fn parse_md5(hex: &str) -> [u8; 16] {
    let bytes = hex.as_bytes();
    assert_eq!(bytes.len(), 32, "expected 32 hex chars, got {hex:?}");
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        let hi = (bytes[i * 2] as char)
            .to_digit(16)
            .unwrap_or_else(|| panic!("bad hex digit in {hex:?}"));
        let lo = (bytes[i * 2 + 1] as char)
            .to_digit(16)
            .unwrap_or_else(|| panic!("bad hex digit in {hex:?}"));
        *byte = ((hi << 4) | lo) as u8;
    }
    out
}

fn format_md5(hash: &[u8; 16]) -> String {
    hash.iter()
        .fold(String::with_capacity(32), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

fn load_stage(stage: &str) -> HashMapByPos {
    let path = format!(
        "{}/test_assets/large_scale_overworld_{stage}.csv.gz",
        env!("CARGO_MANIFEST_DIR"),
    );
    let compressed = fs::read(&path)
        .unwrap_or_else(|error| panic!("failed to read large-scale fixture {path}: {error}"));
    let reader = BufReader::new(GzDecoder::new(Cursor::new(compressed)));

    let mut map: HashMapByPos = FxHashMap::with_capacity_and_hasher(1_000_000, FxBuildHasher);
    for line in reader.lines() {
        let line = line.unwrap_or_else(|error| panic!("failed to read {path}: {error}"));
        let mut fields = line.split(',');
        let mut next = |what: &str| {
            fields
                .next()
                .unwrap_or_else(|| panic!("missing {what} in {path} line {line:?}"))
        };
        let x: i32 = next("chunk_x").parse().expect("chunk_x is not an integer");
        let z: i32 = next("chunk_z").parse().expect("chunk_z is not an integer");
        map.insert((x, z), parse_md5(next("hash")));
    }
    map
}

fn load_fixture() -> Fixture {
    let noise = load_stage("noise");
    let surface = load_stage("surface");
    assert_eq!(
        noise.len(),
        surface.len(),
        "noise and surface fixtures cover a different number of chunks"
    );
    assert!(!noise.is_empty(), "large-scale fixture is empty");

    // The extractor writes the full square [-half, half) on both axes.
    let max_x = noise.keys().map(|&(x, _)| x).max().expect("non-empty");
    let half_size = max_x + 1;
    let side = i64::from(half_size) * 2;
    assert_eq!(
        noise.len() as i64,
        side * side,
        "fixture is not the complete {side}x{side} square the extractor produces"
    );

    Fixture {
        noise,
        surface,
        half_size,
    }
}

fn env_i32(name: &str) -> Option<i32> {
    env::var(name).ok().map(|value| {
        value
            .parse()
            .unwrap_or_else(|_| panic!("{name} is not an integer"))
    })
}

/// Biome lookup in quart coordinates, reading from the tile's chunks. Mirrors
/// the neighbour lookup the surface stage uses in `chunk_stage_hashes`.
#[expect(
    clippy::similar_names,
    reason = "local_qx/local_qz mirror the axis names used across the worldgen code"
)]
fn neighbor_biomes(
    chunks: &FxHashMap<(i32, i32), Chunk>,
    min_qy: i32,
    total_quarts_y: i32,
) -> impl Fn(IVec3) -> u16 + '_ {
    move |q: IVec3| -> u16 {
        let cx = q.x >> 2;
        let cz = q.z >> 2;
        let neighbor = chunks
            .get(&(cx, cz))
            .unwrap_or_else(|| panic!("missing biome-ring chunk ({cx}, {cz})"));
        let local_qx = (q.x - cx * 4) as usize;
        let local_qz = (q.z - cz * 4) as usize;
        let qy_clamped = (q.y - min_qy).clamp(0, total_quarts_y - 1) as usize;
        neighbor.sections.sections[qy_clamped / 4]
            .read()
            .biomes
            .get(local_qx, qy_clamped % 4, local_qz)
    }
}

struct Mismatch {
    pos: (i32, i32),
    stage: &'static str,
    expected: String,
    actual: String,
}

/// Dimension geometry the tile generation needs, derived once from the dimension type.
#[derive(Clone, Copy)]
struct Geometry {
    min_y: i32,
    height: i32,
    section_count: usize,
    min_quart_y: i32,
    total_quarts_y: i32,
}

impl Geometry {
    fn overworld() -> Self {
        let dim_type = &vanilla_dimension_types::OVERWORLD;
        let section_count = (dim_type.height / 16) as usize;
        Self {
            min_y: dim_type.min_y,
            height: dim_type.height,
            section_count,
            min_quart_y: dim_type.min_y >> 2,
            total_quarts_y: (section_count * 4) as i32,
        }
    }
}

/// Generates `[min_x, max_x) x [min_z, max_z)` to NOISE then SURFACE and compares
/// both stages against the fixture, returning whatever didn't match.
fn check_tile(
    generator: &ChunkGeneratorType,
    fixture: &Fixture,
    geometry: Geometry,
    (min_x, min_z): (i32, i32),
    (max_x, max_z): (i32, i32),
) -> Vec<Mismatch> {
    // The tile plus a one-chunk ring: the surface stage reads neighbour biomes.
    let mut chunks: FxHashMap<(i32, i32), Chunk> = FxHashMap::default();
    for x in (min_x - 1)..=max_x {
        for z in (min_z - 1)..=max_z {
            chunks.insert(
                (x, z),
                empty_proto_chunk(
                    (x, z),
                    geometry.section_count,
                    geometry.min_y,
                    geometry.height,
                ),
            );
        }
    }
    for chunk in chunks.values() {
        generator.create_biomes(chunk);
    }

    let mut targets: Vec<(i32, i32)> =
        Vec::with_capacity(((max_x - min_x) * (max_z - min_z)) as usize);
    for x in min_x..max_x {
        for z in min_z..max_z {
            targets.push((x, z));
        }
    }

    let mut mismatches = Vec::new();

    // NOISE — no beardifier, matching how the fixture was extracted.
    for &pos in &targets {
        generator.fill_from_noise(GenerationChunk::<NoisePhase>::for_test(&chunks[&pos]), None);
    }
    collect_mismatches(&chunks, &targets, &fixture.noise, "noise", &mut mismatches);

    // SURFACE
    {
        let lookup = neighbor_biomes(&chunks, geometry.min_quart_y, geometry.total_quarts_y);
        for &pos in &targets {
            generator.build_surface(
                GenerationChunk::<SurfacePhase>::for_test(&chunks[&pos]),
                &lookup,
            );
        }
    }
    collect_mismatches(
        &chunks,
        &targets,
        &fixture.surface,
        "surface",
        &mut mismatches,
    );

    mismatches
}

fn collect_mismatches(
    chunks: &FxHashMap<(i32, i32), Chunk>,
    targets: &[(i32, i32)],
    expected: &HashMapByPos,
    stage: &'static str,
    out: &mut Vec<Mismatch>,
) {
    for &pos in targets {
        let actual = compute_block_hash(&chunks[&pos].sections);
        let expected = expected
            .get(&pos)
            .expect("fixture covers every chunk in the tested square");
        if parse_md5(&actual) != *expected {
            out.push(Mismatch {
                pos,
                stage,
                expected: format_md5(expected),
                actual,
            });
        }
    }
}

#[test]
#[ignore = "generates up to a million chunks; run with --release and STEEL_LARGE_SCALE_HALF_SIZE to scope it"]
fn large_scale_chunk_hashes() {
    use std::panic;
    use std::thread;

    // Matches chunk_stage_hashes: biome pre-generation is stack hungry in debug builds.
    let result = thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(large_scale_chunk_hashes_inner)
        .expect("failed to spawn large-scale test thread")
        .join();

    if let Err(payload) = result {
        panic::resume_unwind(payload);
    }
}

fn large_scale_chunk_hashes_inner() {
    use crate::bootstrap::init_globals_once;
    use crate::worldgen::OverworldGenerator;

    init_globals_once();

    let fixture = load_fixture();
    let half_size = env_i32(HALF_SIZE_ENV).map_or(fixture.half_size, |requested| {
        assert!(
            requested > 0 && requested <= fixture.half_size,
            "{HALF_SIZE_ENV} must be in 1..={}",
            fixture.half_size
        );
        requested
    });
    let max_mismatches =
        env_i32(MAX_MISMATCHES_ENV).map_or(DEFAULT_MAX_MISMATCHES, |value| value.max(1) as usize);

    let geometry = Geometry::overworld();

    let thread_pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .thread_name(|index| format!("large-scale-hashes-{index}"))
            .build()
            .expect("failed to create large-scale test rayon pool"),
    );
    let generator: Arc<ChunkGeneratorType> = Arc::new(ChunkGeneratorType::Overworld(
        OverworldGenerator::new(None, BiomeSourceKind::overworld(SEED), SEED, &thread_pool),
    ));

    let side = i64::from(half_size) * 2;
    let total = side * side;
    eprintln!(
        "[large-scale] overworld seed {SEED}: {side}x{side} = {total} chunks (tile {TILE_SIZE})"
    );

    let mut checked = 0i64;
    let mut mismatches: Vec<Mismatch> = Vec::new();

    let mut tile_x = -half_size;
    'tiles: while tile_x < half_size {
        let max_x = (tile_x + TILE_SIZE).min(half_size);
        let mut tile_z = -half_size;
        while tile_z < half_size {
            let max_z = (tile_z + TILE_SIZE).min(half_size);

            mismatches.extend(check_tile(
                &generator,
                &fixture,
                geometry,
                (tile_x, tile_z),
                (max_x, max_z),
            ));
            checked += i64::from(max_x - tile_x) * i64::from(max_z - tile_z);
            eprintln!(
                "[large-scale] {checked}/{total} chunks, {} mismatches",
                mismatches.len()
            );

            if mismatches.len() >= max_mismatches {
                break 'tiles;
            }
            tile_z += TILE_SIZE;
        }
        tile_x += TILE_SIZE;
    }

    if !mismatches.is_empty() {
        let mut report = String::new();
        for mismatch in mismatches.iter().take(max_mismatches) {
            let _ = write!(
                report,
                "\n  ({:6},{:6}) {:7} expected={} actual={}",
                mismatch.pos.0, mismatch.pos.1, mismatch.stage, mismatch.expected, mismatch.actual
            );
        }
        panic!(
            "{} of {checked} checked chunks mismatched the vanilla fixture:{report}",
            mismatches.len()
        );
    }

    eprintln!("[large-scale] all {checked} chunks match");
}
