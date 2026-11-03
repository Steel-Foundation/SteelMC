//! Large-scale Terrain regression test.
//!
//! Compares Steel's Terrain step (noise fill, surface, carvers) against
//! per-chunk MD5 hashes that `SteelExtractor`'s `LargeScaleChunkHashes` produced
//! for a contiguous 1000×1000 chunk square in every dimension.
//! `chunk_stage_hashes` samples 2,500 chunks per dimension; this fixture covers
//! one million each, which catches rare terrain, surface-rule and carver paths
//! the sample misses.
//!
//! The extractor deliberately skips `STRUCTURE_STARTS`/`STRUCTURE_REFERENCES`, so
//! the chunks carry no structure references and terrain is generated without a
//! `Beardifier`. Only the atomic TERRAIN step is hashed; features and light are
//! out of scope.
//!
//! Fixtures: `test_assets/large_scale_{dimension}_terrain.csv.gz`, gzipped
//! `chunk_x,chunk_z,md5` rows. They are gitignored; regenerate them with the
//! extractor's `STEEL_EXTRACTOR_LARGE_SCALE_HALF_SIZE=500` mode and
//! `STEEL_EXTRACTOR_LARGE_SCALE_DIMENSIONS=overworld,the_nether,the_end` on a
//! `level-seed=13579` world. Dimensions without a fixture file are skipped.
//!
//! When `test_assets/large_scale_{dimension}_debug_terrain_blocks.bin.gz` exists
//! (the extractor's focused dump of specific chunks, same binary layout as the
//! chunk-stage dumps), mismatching chunks it covers are reported block by block.

use std::env;
use std::fmt::Write;
use std::fs;
use std::io::{Cursor, ErrorKind, Read as IoRead};
use std::str;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use flate2::read::GzDecoder;
use glam::IVec3;
use rayon::prelude::*;
use rustc_hash::{FxBuildHasher, FxHashMap};
use steel_registry::vanilla_dimension_types;
use steel_utils::locks::SyncMutex;
use steel_worldgen::biomes::BiomeSourceKind;

use super::chunk_stage_hashes::{
    ChunkBlockData, chunk_or_panic, compute_block_hash, diff_chunk, empty_proto_chunk,
    format_chunk_diffs, read_reference_blocks,
};
use crate::chunk::Chunk;
use crate::worldgen::generator::{GenerationChunk, TerrainPhase};
use crate::worldgen::{
    ChunkGenerator, ChunkGeneratorType, EndGenerator, NetherGenerator, OverworldGenerator,
};

/// Seed of the world the extractor loaded, `STEEL_LARGE_SCALE_SEED` (default
/// 13579). Fixtures of the default seed are `large_scale_{dim}_{stage}.csv.gz`;
/// any other seed's carry a `_seed{seed}` suffix (the extractor's
/// `STEEL_EXTRACTOR_LARGE_SCALE_SEED_SUFFIX`). `LargeScaleChunkHashes` hashes
/// the server's live world, so the extractor run must have used that `level-seed`.
const DEFAULT_SEED: u64 = 13579;
const SEED_ENV: &str = "STEEL_LARGE_SCALE_SEED";

fn seed() -> u64 {
    static SEED: OnceLock<u64> = OnceLock::new();
    *SEED.get_or_init(|| {
        env::var(SEED_ENV).map_or(DEFAULT_SEED, |value| {
            let value = value.trim();
            value
                .parse::<i64>()
                .unwrap_or_else(|error| panic!("{SEED_ENV}={value:?} is not an i64: {error}"))
                as u64
        })
    })
}

/// `""` for the default seed, `_seed{seed}` otherwise.
fn seed_suffix() -> String {
    if seed() == DEFAULT_SEED {
        String::new()
    } else {
        format!("_seed{}", seed() as i64)
    }
}

/// Side length in chunks of one generation tile. Each tile allocates the tile
/// plus a one-chunk biome ring, so memory is bounded by the tile, not the area.
const TILE_SIZE: i32 = 16;

/// Restrict the run to the centered square `[-half, half)` for quick checks.
const HALF_SIZE_ENV: &str = "STEEL_LARGE_SCALE_HALF_SIZE";
/// Test a single chunk, formatted as `<chunk_x>,<chunk_z>`.
const DEBUG_CHUNK_ENV: &str = "STEEL_HASH_DEBUG_CHUNK";
/// Test a single dimension, e.g. `minecraft:the_nether`.
const DEBUG_DIMENSION_ENV: &str = "STEEL_HASH_DEBUG_DIMENSION";
const STOP_AFTER_FIRST_MISMATCH_ENV: &str = "STEEL_HASH_STOP_AFTER_FIRST_MISMATCH";

/// Mismatches listed per dimension in the failure message before truncating.
const MAX_REPORTED_MISMATCHES: usize = 50;
/// Chunks generated between progress lines.
const PROGRESS_INTERVAL: u64 = 20_000;

const STAGE: &str = "terrain";

/// Dimension order for deterministic test output.
const DIMENSION_ORDER: &[&str] = &[
    "minecraft:overworld",
    "minecraft:the_nether",
    "minecraft:the_end",
];

type ChunkHashes = FxHashMap<(i32, i32), [u8; 16]>;

fn fixture_path(dim_short: &str) -> String {
    format!(
        "{}/test_assets/large_scale_{dim_short}_{STAGE}{}.csv.gz",
        env!("CARGO_MANIFEST_DIR"),
        seed_suffix(),
    )
}

/// Reference block dump for specific chunks, used to explain mismatches.
fn load_debug_reference_blocks(dim_short: &str) -> Option<FxHashMap<(i32, i32), ChunkBlockData>> {
    let path = format!(
        "{}/test_assets/large_scale_{dim_short}_debug_{STAGE}_blocks{}.bin.gz",
        env!("CARGO_MANIFEST_DIR"),
        seed_suffix(),
    );
    read_reference_blocks(&path)
}

fn parse_md5_hex(hex: &str) -> Option<[u8; 16]> {
    if hex.len() != 32 {
        return None;
    }
    let mut digest = [0u8; 16];
    let (pairs, _) = hex.as_bytes().as_chunks::<2>();
    for (byte, pair) in digest.iter_mut().zip(pairs) {
        *byte = u8::from_str_radix(str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(digest)
}

/// Loads a dimension's fixture, or `None` when no fixture file exists for it.
fn load_fixture(dim_short: &str) -> Option<ChunkHashes> {
    let path = fixture_path(dim_short);
    let compressed = match fs::read(&path) {
        Ok(compressed) => compressed,
        Err(error) if error.kind() == ErrorKind::NotFound => return None,
        Err(error) => panic!("failed to read large-scale fixture {path}: {error}"),
    };
    let mut text = String::new();
    GzDecoder::new(Cursor::new(compressed))
        .read_to_string(&mut text)
        .unwrap_or_else(|error| panic!("failed to decompress {path}: {error}"));

    let mut hashes = ChunkHashes::with_capacity_and_hasher(text.lines().count(), FxBuildHasher);
    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        let mut fields = line.split(',');
        let (Some(x), Some(z), Some(hex), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            panic!("{path}:{line_number}: expected `chunk_x,chunk_z,md5`, got {line:?}");
        };
        let Ok(x) = x.parse::<i32>() else {
            panic!("{path}:{line_number}: chunk_x is not an i32: {x}");
        };
        let Ok(z) = z.parse::<i32>() else {
            panic!("{path}:{line_number}: chunk_z is not an i32: {z}");
        };
        let Some(digest) = parse_md5_hex(hex) else {
            panic!("{path}:{line_number}: not a 32-digit hex MD5: {hex}");
        };
        let previous = hashes.insert((x, z), digest);
        assert!(
            previous.is_none(),
            "{path}:{line_number}: duplicate chunk ({x}, {z})"
        );
    }
    Some(hashes)
}

fn parse_chunk_env(name: &str) -> Option<(i32, i32)> {
    let value = env::var(name).ok()?;
    let Some((x, z)) = value.split_once(',') else {
        panic!("{name} must be formatted as '<chunk_x>,<chunk_z>'");
    };
    let Ok(chunk_x) = x.parse::<i32>() else {
        panic!("{name} chunk_x is not an i32: {x}");
    };
    let Ok(chunk_z) = z.parse::<i32>() else {
        panic!("{name} chunk_z is not an i32: {z}");
    };
    Some((chunk_x, chunk_z))
}

fn parse_half_size_env() -> Option<i32> {
    let value = env::var(HALF_SIZE_ENV).ok()?;
    let Ok(half_size) = value.parse::<i32>() else {
        panic!("{HALF_SIZE_ENV} is not an i32: {value}");
    };
    assert!(half_size > 0, "{HALF_SIZE_ENV} must be positive");
    Some(half_size)
}

fn debug_dimension_filter() -> Option<String> {
    env::var(DEBUG_DIMENSION_ENV)
        .ok()
        .filter(|dimension| !dimension.is_empty())
}

/// Chunk positions selected by the fixture and the debug environment filters,
/// grouped into generation tiles. Tiles and their chunks are sorted so the run
/// order is deterministic regardless of `HashMap` iteration.
fn select_tiles(fixture: &ChunkHashes) -> Vec<Vec<(i32, i32)>> {
    let half_size = parse_half_size_env();
    let debug_chunk = parse_chunk_env(DEBUG_CHUNK_ENV);

    let mut tiles: FxHashMap<(i32, i32), Vec<(i32, i32)>> = FxHashMap::default();
    for &(x, z) in fixture.keys() {
        if half_size.is_some_and(|half| x < -half || x >= half || z < -half || z >= half) {
            continue;
        }
        if debug_chunk.is_some_and(|debug| debug != (x, z)) {
            continue;
        }
        tiles
            .entry((x.div_euclid(TILE_SIZE), z.div_euclid(TILE_SIZE)))
            .or_default()
            .push((x, z));
    }

    let mut tiles: Vec<_> = tiles.into_iter().collect();
    tiles.sort_unstable_by_key(|(tile, _)| *tile);
    tiles
        .into_iter()
        .map(|(_, mut chunks)| {
            chunks.sort_unstable();
            chunks
        })
        .collect()
}

/// Runs BIOMES for the tile and its one-chunk ring, then TERRAIN for the tile's
/// chunks and hashes each — the same sequence `LargeScaleChunkHashes.processTile`
/// runs in vanilla.
fn generate_tile(
    generator: &ChunkGeneratorType,
    targets: &[(i32, i32)],
    min_y: i32,
    height: i32,
) -> Vec<((i32, i32), md5::Digest)> {
    let chunks = generate_tile_chunks(generator, targets, min_y, height);
    targets
        .iter()
        .map(|&pos| {
            (
                pos,
                compute_block_hash(&chunk_or_panic(&chunks, pos).sections),
            )
        })
        .collect()
}

/// Generates terrain for `targets` and returns them together with their biome
/// ring. Steel's carvers sample source-chunk biomes from the biome source
/// directly, so the radius-8 ring vanilla's `carverBiome` needs is not allocated.
fn generate_tile_chunks(
    generator: &ChunkGeneratorType,
    targets: &[(i32, i32)],
    min_y: i32,
    height: i32,
) -> FxHashMap<(i32, i32), Chunk> {
    let section_count = (height / 16) as usize;
    let min_quart_y = min_y >> 2;
    let total_quarts_y = (section_count * 4) as i32;

    let (mut min_x, mut max_x, mut min_z, mut max_z) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    for &(x, z) in targets {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_z = min_z.min(z);
        max_z = max_z.max(z);
    }

    let mut chunks: FxHashMap<(i32, i32), Chunk> = FxHashMap::default();
    for x in (min_x - 1)..=(max_x + 1) {
        for z in (min_z - 1)..=(max_z + 1) {
            chunks.insert(
                (x, z),
                empty_proto_chunk((x, z), section_count, min_y, height),
            );
        }
    }
    for chunk in chunks.values() {
        generator.create_biomes(chunk);
    }

    let neighbor_biomes = |quart_pos: IVec3| -> u16 {
        let neighbor_chunk_x = quart_pos.x >> 2;
        let neighbor_chunk_z = quart_pos.z >> 2;
        let neighbor = chunk_or_panic(&chunks, (neighbor_chunk_x, neighbor_chunk_z));
        let local_quart_x = (quart_pos.x - neighbor_chunk_x * 4) as usize;
        let local_quart_z = (quart_pos.z - neighbor_chunk_z * 4) as usize;
        let local_quart_y = (quart_pos.y - min_quart_y).clamp(0, total_quarts_y - 1) as usize;
        let section_index = local_quart_y / 4;
        neighbor.sections.sections[section_index].read().biomes.get(
            local_quart_x,
            local_quart_y % 4,
            local_quart_z,
        )
    };

    for &pos in targets {
        generator.build_terrain(
            GenerationChunk::<TerrainPhase>::for_test(chunk_or_panic(&chunks, pos)),
            None,
            &neighbor_biomes,
        );
    }
    chunks
}

struct Mismatch {
    pos: (i32, i32),
    expected: [u8; 16],
    actual: md5::Digest,
}

#[derive(Default)]
struct Mismatches {
    total: usize,
    shown: Vec<Mismatch>,
}

impl Mismatches {
    fn push(&mut self, mismatch: Mismatch) {
        self.total += 1;
        if self.shown.len() < MAX_REPORTED_MISMATCHES {
            self.shown.push(mismatch);
        }
    }

    /// Formats the failure report. `block_diffs` returns the block-level report for
    /// a chunk covered by the debug dump; other chunks are listed by hash only.
    fn format(
        &self,
        dim_short: &str,
        total_chunks: usize,
        has_reference: bool,
        block_diffs: impl Fn((i32, i32)) -> Option<String>,
    ) -> String {
        let mut msg = format!(
            "{dim_short}/{STAGE}: {}/{total_chunks} chunks do not match vanilla",
            self.total
        );
        if !has_reference {
            msg.push_str(" (no binary reference data, showing hashes only)");
        }
        msg.push('\n');
        for mismatch in &self.shown {
            if let Some(diffs) = block_diffs(mismatch.pos) {
                msg.push_str(&diffs);
                continue;
            }
            let _ = writeln!(
                msg,
                "  ({:4},{:4}): expected {:x}, got {:x}",
                mismatch.pos.0,
                mismatch.pos.1,
                md5::Digest(mismatch.expected),
                mismatch.actual,
            );
        }
        if self.total > self.shown.len() {
            let _ = writeln!(msg, "  ... and {} more", self.total - self.shown.len());
        }
        msg
    }
}

struct Progress<'a> {
    dim_short: &'a str,
    generated: AtomicU64,
    total: u64,
    started: Instant,
}

impl Progress<'_> {
    fn advance(&self, chunks: u64) {
        let before = self.generated.fetch_add(chunks, Ordering::Relaxed);
        let after = before + chunks;
        if before / PROGRESS_INTERVAL == after / PROGRESS_INTERVAL && after != self.total {
            return;
        }
        let elapsed = self.started.elapsed().as_secs_f64();
        let rate = after as f64 / elapsed;
        let remaining = (self.total - after) as f64 / rate;
        eprintln!(
            "[{}] {after}/{} chunks ({rate:.1}/s, {elapsed:.0}s elapsed, ~{remaining:.0}s remaining)",
            self.dim_short, self.total
        );
    }
}

/// Compares one chunk against the fixture, recording and printing a mismatch.
fn check_chunk(
    dim_short: &str,
    pos: (i32, i32),
    fixture: &ChunkHashes,
    actual: md5::Digest,
    mismatches: &SyncMutex<Mismatches>,
) -> bool {
    let Some(&expected) = fixture.get(&pos) else {
        panic!(
            "chunk ({}, {}) is missing from the {dim_short} fixture",
            pos.0, pos.1
        );
    };
    if actual.0 == expected {
        return true;
    }
    eprintln!(
        "[{dim_short}/{STAGE}] ({:4},{:4}) MISMATCH expected={:x} actual={actual:x}",
        pos.0,
        pos.1,
        md5::Digest(expected),
    );
    mismatches.lock().push(Mismatch {
        pos,
        expected,
        actual,
    });
    false
}

#[test]
#[ignore = "generates up to one million chunks per dimension; run with --release"]
fn large_scale_hashes() {
    use std::panic;
    use std::thread;

    // Larger stack to match chunk_stage_hashes.rs — generation recurses deeply in
    // debug builds.
    let result = thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(large_scale_hashes_inner)
        .expect("Failed to spawn test thread")
        .join();

    if let Err(payload) = result {
        panic::resume_unwind(payload);
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one pass per dimension: fixture load, generator setup, sweep, report"
)]
fn large_scale_hashes_inner() {
    use crate::bootstrap::init_globals;

    init_globals();

    let debug_dimension = debug_dimension_filter();
    let stop_after_first_mismatch = env::var_os(STOP_AFTER_FIRST_MISMATCH_ENV).is_some();
    let tile_pool = rayon::ThreadPoolBuilder::new()
        .stack_size(16 * 1024 * 1024)
        .thread_name(|index| format!("large-scale-hashes-{index}"))
        .build()
        .expect("failed to create large-scale hash test tile pool");
    let mut checked_dimensions = 0usize;

    for &dim_key in DIMENSION_ORDER {
        if debug_dimension
            .as_deref()
            .is_some_and(|filter| filter != dim_key)
        {
            continue;
        }
        let dim_short = dim_key.strip_prefix("minecraft:").unwrap_or(dim_key);
        let Some(fixture) = load_fixture(dim_short) else {
            eprintln!("[{dim_short}] no large-scale fixture, skipping");
            continue;
        };
        checked_dimensions += 1;

        let tiles = select_tiles(&fixture);
        let total_chunks: usize = tiles.iter().map(Vec::len).sum();
        assert!(total_chunks > 0, "[{dim_short}] no fixture chunks selected");
        eprintln!(
            "[{dim_short}] {total_chunks} chunks in {} tiles (fixture: {} chunks, seed {})",
            tiles.len(),
            fixture.len(),
            seed() as i64
        );

        let dim_type = match dim_key {
            "minecraft:overworld" => &vanilla_dimension_types::OVERWORLD,
            "minecraft:the_nether" => &vanilla_dimension_types::THE_NETHER,
            "minecraft:the_end" => &vanilla_dimension_types::THE_END,
            _ => panic!("Unknown dimension: {dim_key}"),
        };

        // The generator only uses its pool while constructing biome and structure data.
        let generator_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .thread_name(|index| format!("large-scale-hashes-init-{index}"))
            .build()
            .expect("failed to create large-scale hash test init pool");
        let generator = match dim_key {
            "minecraft:overworld" => ChunkGeneratorType::Overworld(OverworldGenerator::new(
                None,
                BiomeSourceKind::overworld(seed()),
                seed(),
                &generator_pool,
            )),
            "minecraft:the_nether" => ChunkGeneratorType::Nether(NetherGenerator::new(
                None,
                BiomeSourceKind::nether(seed()),
                seed(),
                &generator_pool,
            )),
            "minecraft:the_end" => ChunkGeneratorType::End(EndGenerator::new(
                None,
                BiomeSourceKind::end(seed()),
                seed(),
                &generator_pool,
            )),
            _ => unreachable!(),
        };

        let stop = AtomicBool::new(false);
        let progress = Progress {
            dim_short,
            generated: AtomicU64::new(0),
            total: total_chunks as u64,
            started: Instant::now(),
        };
        let mismatches = SyncMutex::new(Mismatches::default());

        tile_pool.install(|| {
            tiles.par_iter().for_each(|tile| {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let mut tile_ok = true;
                for (pos, actual) in
                    generate_tile(&generator, tile, dim_type.min_y, dim_type.height)
                {
                    tile_ok &= check_chunk(dim_short, pos, &fixture, actual, &mismatches);
                }
                if !tile_ok && stop_after_first_mismatch {
                    stop.store(true, Ordering::Relaxed);
                }
                progress.advance(tile.len() as u64);
            });
        });

        let mismatches = mismatches.into_inner();
        if mismatches.total == 0 {
            continue;
        }
        let reference_blocks = load_debug_reference_blocks(dim_short);
        let block_diffs = |pos: (i32, i32)| {
            let reference = reference_blocks.as_ref()?.get(&pos)?;
            let chunks = generate_tile_chunks(&generator, &[pos], dim_type.min_y, dim_type.height);
            let chunk = chunk_or_panic(&chunks, pos);
            let diffs = diff_chunk(&chunk.sections, reference, dim_type.min_y);
            (!diffs.is_empty()).then(|| format_chunk_diffs(&diffs, pos.0, pos.1, dim_type.min_y))
        };
        panic!(
            "{}",
            mismatches.format(
                dim_short,
                total_chunks,
                reference_blocks.is_some(),
                block_diffs
            )
        );
    }

    assert!(
        checked_dimensions > 0,
        "no large-scale fixtures found; produce them with SteelExtractor's \
         STEEL_EXTRACTOR_LARGE_SCALE_HALF_SIZE mode and copy them into test_assets"
    );
}
