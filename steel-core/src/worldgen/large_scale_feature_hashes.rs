//! Large-scale structure and feature regression test.
//!
//! Compares Steel's `STRUCTURE_STARTS`, `TERRAIN` (with a live beardifier) and
//! `FEATURES` output against per-chunk fixtures that `SteelExtractor`'s
//! `LargeScaleChunkHashes` features mode produced for a contiguous square of
//! chunks (1000×1000 in the full runs) in every dimension. `large_scale_hashes`
//! covers terrain without structures; this covers the long tail of structure
//! and feature code paths the 2,500-chunk sample misses.
//!
//! Semantics, mirrored from the extractor:
//! * The square is swept in x bands. Structure starts exist for a radius-8 ring
//!   around everything that is generated further, biomes one chunk outside the
//!   terrain, terrain one chunk outside the square.
//! * Features run in global x-then-z ascending order and only inside the
//!   square; the ring gets terrain but never features, like the ring around a
//!   chunk-stage cluster. Vanilla runs starts, references and features
//!   consecutively, so they are sequential here too.
//! * `terrain_with_structures` is the block hash right after TERRAIN, before any
//!   neighbor's features; `features` is the hash once every neighbor inside
//!   the square has run its features; `structure_starts` lists every start by
//!   its origin chunk.
//!
//! Fixtures: `test_assets/large_scale_{dim}_{terrain_with_structures,features,
//! structure_starts}.csv.gz`, gitignored. Regenerate them with
//! `STEEL_EXTRACTOR_LARGE_SCALE_HALF_SIZE=500 STEEL_EXTRACTOR_LARGE_SCALE_FEATURES=1`
//! on a `level-seed=13579` world. Dimensions without a features fixture are
//! skipped; the other two fixtures are optional.
//!
//! When `test_assets/large_scale_{dim}_debug_{stage}_blocks.bin.gz` exists (the
//! extractor's `STEEL_EXTRACTOR_LARGE_SCALE_DEBUG_CHUNKS` dump), mismatching
//! chunks it covers are reported block by block.
//!
//! `KNOWN_DIVERGENCES` lists understood divergences by seed, dimension, stage, chunk
//! and exact vanilla value; anything else in those chunks still fails.
//!
//! Env: `STEEL_LARGE_SCALE_HALF_SIZE` restricts the sweep to a centered square
//! (features near its edge then legitimately differ, so only chunks at least
//! `RESTRICTED_MARGIN` inside are compared), `STEEL_LARGE_SCALE_BAND` sets the
//! band width, `STEEL_LARGE_SCALE_STOP_X` ends the sweep once the column with
//! that x is final (cheap reruns against a debug dump that stops there too),
//! `STEEL_HASH_DEBUG_DIMENSION` picks one dimension and
//! `STEEL_HASH_STOP_AFTER_FIRST_MISMATCH` stops after the first bad band.

use std::env;
use std::fmt::Write;
use std::fs;
use std::io::{Cursor, ErrorKind, Read as IoRead};
use std::str;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use flate2::read::GzDecoder;
use glam::IVec3;
use rayon::prelude::*;
use rustc_hash::{FxBuildHasher, FxHashMap, FxHashSet};
use steel_registry::structure::TerrainAdjustment;
use steel_registry::vanilla_dimension_types;
use steel_utils::ChunkPos;
use steel_worldgen::biomes::BiomeSourceKind;
use steel_worldgen::noise::Beardifier;
use steel_worldgen::structure::StructureStart;

use super::chunk_stage_hashes::{
    ChunkBlockData, compute_block_hash, create_test_world, diff_chunk, empty_proto_chunk,
    format_chunk_diffs, read_reference_blocks,
};
use crate::chunk::Chunk;
use crate::chunk::chunk_generation_task::StaticCache2D;
use crate::chunk::chunk_holder::ChunkHolder;
use crate::chunk::chunk_pyramid::{ChunkStep, GENERATION_PYRAMID};
use crate::chunk::chunk_ticket_manager::ChunkTicketLevel;
use crate::chunk::status::ChunkStatus;
use crate::entity::damage::DamageHistory;
use crate::worldgen::generator::{GenerationChunk, TerrainPhase};
use crate::worldgen::{
    ChunkGenerator, ChunkGeneratorType, EndGenerator, NetherGenerator, OverworldGenerator,
    WorldGenContext, WorldGenRegion,
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
/// Radius of the widest generation dependency: structure references and placement
/// look 8 chunks out, matching the extractor's starts ring.
const DEPENDENCY_RADIUS: i32 = 8;
const DEFAULT_BAND_WIDTH: i32 = 32;
/// Features reach two chunks through a neighbor's writes, so a restricted sweep
/// cannot reproduce the fixture closer than this to its edge.
const RESTRICTED_MARGIN: i32 = 3;
/// Mismatches listed per stage in the failure message before truncating.
const MAX_REPORTED_MISMATCHES: usize = 50;

const HALF_SIZE_ENV: &str = "STEEL_LARGE_SCALE_HALF_SIZE";
const BAND_ENV: &str = "STEEL_LARGE_SCALE_BAND";
const STOP_X_ENV: &str = "STEEL_LARGE_SCALE_STOP_X";
const DEBUG_DIMENSION_ENV: &str = "STEEL_HASH_DEBUG_DIMENSION";
const STOP_AFTER_FIRST_MISMATCH_ENV: &str = "STEEL_HASH_STOP_AFTER_FIRST_MISMATCH";

const TERRAIN_STAGE: &str = "terrain_with_structures";
const FEATURES_STAGE: &str = "features";
const STARTS_STAGE: &str = "structure_starts";

/// Dimension order for deterministic test output.
const DIMENSION_ORDER: &[&str] = &[
    "minecraft:overworld",
    "minecraft:the_nether",
    "minecraft:the_end",
];

/// A vanilla divergence that is understood and deliberately not fixed yet.
///
/// An entry only matches its seed, its exact chunk and the exact vanilla value from
/// the fixture, so regenerated fixtures or a new divergence in the same chunk still fail.
struct KnownDivergence {
    seed: i64,
    dimension: &'static str,
    stage: &'static str,
    pos: (i32, i32),
    /// The fixture value as printed in the mismatch line (md5 hex, or the
    /// `+`-joined structure ids for starts).
    expected: &'static str,
    reason: &'static str,
}

/// Steel's `BlockBehavior::update_shape` has no random source, so growing-plant
/// body→head conversion at template edges cannot draw vanilla's
/// `WorldGenRegion.getRandom()` age.
const VINE_AGE_REASON: &str = "growing plant head age from the worldgen region random";

// TODO: Remove the entry once villagers are implemented. Vanilla finalizes the village's
// template villagers with `WorldGenRegion.getRandom()` (3 draws each); Steel cannot
// create them yet, so the chunk's later region-random draws (moss carpet toppers) shift.
const TEMPLATE_VILLAGER_REASON: &str = "template villager finalizeSpawn region-random draws";

const KNOWN_DIVERGENCES: &[KnownDivergence] = &[
    KnownDivergence::features(
        13579,
        "the_nether",
        (180, -177),
        "56f571730975c68a4038404dea21a2a3",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        13579,
        "the_nether",
        (296, -32),
        "4540e664c3552fd8e8e4dcc5b063d915",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        13579,
        "the_nether",
        (300, -317),
        "e9c74159c23576fcdfee486ba16a1f3b",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        0,
        "the_nether",
        (-103, 256),
        "2cdc54775f0f900fd9ceb5cdc06114df",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        0,
        "the_nether",
        (-96, -113),
        "6ce614ec1954b1ce09cbc4e98894c799",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        123_456_789,
        "the_nether",
        (-425, -304),
        "e3160eb995db89460c1ccd22350eff3d",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        123_456_789,
        "the_nether",
        (-395, 287),
        "b771fb3f96f496f705bc5dd5f08e2248",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        123_456_789,
        "the_nether",
        (-271, -143),
        "b4afd8118772fe3b537f84586015cc56",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        31337,
        "the_nether",
        (365, -195),
        "da6e4d4a742461c520812732641928eb",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        31337,
        "overworld",
        (-417, 12),
        "dfdc6e0c65c5452d8817ee810bd29574",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        31337,
        "overworld",
        (-80, 323),
        "90487ab343dd6758f8c45ec36da006cb",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        31337,
        "overworld",
        (95, 42),
        "de50c7ff54b506e91a6e44f611010a91",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        42,
        "the_nether",
        (-304, 48),
        "8f7d787d63d59cd013d59bd26caf264e",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        42,
        "the_nether",
        (-302, 323),
        "f998382c7ec0a49774cd43a3a0e8647c",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        42,
        "the_nether",
        (240, -120),
        "e3a2319073a4fdc0f3cd25f3e7a31d89",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        42,
        "overworld",
        (-454, -215),
        "8e8f4914e7096f0303db86e5a053f299",
        TEMPLATE_VILLAGER_REASON,
    ),
    KnownDivergence::features(
        7_777_777_777_777_777,
        "the_nether",
        (-359, 174),
        "dec8f2be0aa1c656c8e4123b6e2171e6",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        -8_675_309,
        "the_nether",
        (-263, -394),
        "1edfaeeb23bdfed7abbfa0cb1e68bf26",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        -8_675_309,
        "the_nether",
        (16, -122),
        "c395b91e395f0cab506ea570d3bfc23d",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        -987_654_321_012,
        "the_nether",
        (19, 164),
        "9ad06642291133f861ea455b331baa87",
        VINE_AGE_REASON,
    ),
    KnownDivergence::features(
        -987_654_321_012,
        "the_nether",
        (116, -256),
        "9792fd1597078680f26445a5530ae575",
        VINE_AGE_REASON,
    ),
];

impl KnownDivergence {
    const fn features(
        seed: i64,
        dimension: &'static str,
        pos: (i32, i32),
        expected: &'static str,
        reason: &'static str,
    ) -> Self {
        Self {
            seed,
            dimension,
            stage: FEATURES_STAGE,
            pos,
            expected,
            reason,
        }
    }

    fn find(dimension: &str, stage: &str, pos: (i32, i32)) -> Option<&'static Self> {
        KNOWN_DIVERGENCES.iter().find(|known| {
            known.seed as u64 == seed()
                && known.dimension == dimension
                && known.stage == stage
                && known.pos == pos
        })
    }
}

type ChunkHashes = FxHashMap<(i32, i32), [u8; 16]>;
/// Sorted structure ids per origin chunk.
type ChunkStarts = FxHashMap<(i32, i32), Vec<String>>;
type HolderMap = FxHashMap<(i32, i32), Arc<ChunkHolder>>;

fn fixture_path(dim_short: &str, stage: &str) -> String {
    format!(
        "{}/test_assets/large_scale_{dim_short}_{stage}{}.csv.gz",
        env!("CARGO_MANIFEST_DIR"),
        seed_suffix(),
    )
}

/// Reads a gzipped fixture, or `None` when no such file exists.
fn read_fixture_text(path: &str) -> Option<String> {
    let compressed = match fs::read(path) {
        Ok(compressed) => compressed,
        Err(error) if error.kind() == ErrorKind::NotFound => return None,
        Err(error) => panic!("failed to read large-scale fixture {path}: {error}"),
    };
    let mut text = String::new();
    GzDecoder::new(Cursor::new(compressed))
        .read_to_string(&mut text)
        .unwrap_or_else(|error| panic!("failed to decompress {path}: {error}"));
    Some(text)
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

fn parse_chunk_fields<'a>(path: &str, line_number: usize, line: &'a str) -> (i32, i32, &'a str) {
    let mut fields = line.split(',');
    let (Some(x), Some(z), Some(value), None) =
        (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        panic!("{path}:{line_number}: expected `chunk_x,chunk_z,value`, got {line:?}");
    };
    let Ok(x) = x.parse::<i32>() else {
        panic!("{path}:{line_number}: chunk_x is not an i32: {x}");
    };
    let Ok(z) = z.parse::<i32>() else {
        panic!("{path}:{line_number}: chunk_z is not an i32: {z}");
    };
    (x, z, value)
}

fn load_hashes(dim_short: &str, stage: &str) -> Option<ChunkHashes> {
    let path = fixture_path(dim_short, stage);
    let text = read_fixture_text(&path)?;
    let mut hashes = ChunkHashes::with_capacity_and_hasher(text.lines().count(), FxBuildHasher);
    for (index, line) in text.lines().enumerate() {
        let (x, z, hex) = parse_chunk_fields(&path, index + 1, line);
        let Some(digest) = parse_md5_hex(hex) else {
            panic!("{path}:{}: not a 32-digit hex MD5: {hex}", index + 1);
        };
        assert!(
            hashes.insert((x, z), digest).is_none(),
            "{path}:{}: duplicate chunk ({x}, {z})",
            index + 1
        );
    }
    Some(hashes)
}

fn load_starts(dim_short: &str) -> Option<ChunkStarts> {
    let path = fixture_path(dim_short, STARTS_STAGE);
    let text = read_fixture_text(&path)?;
    let mut starts = ChunkStarts::default();
    for (index, line) in text.lines().enumerate() {
        let (x, z, id) = parse_chunk_fields(&path, index + 1, line);
        starts.entry((x, z)).or_default().push(id.to_owned());
    }
    for ids in starts.values_mut() {
        ids.sort_unstable();
    }
    Some(starts)
}

/// Half side of the square a fixture covers, checking it really is the full
/// `[-half, half)` square the extractor sweeps.
fn square_half_size(fixture: &ChunkHashes, dim_short: &str, stage: &str) -> i32 {
    let max_x = fixture.keys().map(|&(x, _)| x).max().unwrap_or(0);
    let half = max_x + 1;
    let side = u64::try_from(half).unwrap_or(0) * 2;
    assert_eq!(
        fixture.len() as u64,
        side * side,
        "[{dim_short}/{stage}] fixture is not a full [-{half}, {half}) square"
    );
    for &(x, z) in fixture.keys() {
        assert!(
            x >= -half && x < half && z >= -half && z < half,
            "[{dim_short}/{stage}] chunk ({x}, {z}) lies outside [-{half}, {half})"
        );
    }
    half
}

fn parse_env_i32(name: &str) -> Option<i32> {
    let value = env::var(name).ok()?;
    let Ok(parsed) = value.parse::<i32>() else {
        panic!("{name} is not an i32: {value}");
    };
    assert!(parsed > 0, "{name} must be positive");
    Some(parsed)
}

fn parse_env_any_i32(name: &str) -> Option<i32> {
    let value = env::var(name).ok()?;
    let Ok(parsed) = value.parse::<i32>() else {
        panic!("{name} is not an i32: {value}");
    };
    Some(parsed)
}

fn debug_dimension_filter() -> Option<String> {
    env::var(DEBUG_DIMENSION_ENV)
        .ok()
        .filter(|dimension| !dimension.is_empty())
}

/// Inclusive chunk rectangle.
#[derive(Clone, Copy)]
struct Rect {
    x0: i32,
    x1: i32,
    z0: i32,
    z1: i32,
}

impl Rect {
    const fn grow(self, by: i32) -> Self {
        Self {
            x0: self.x0 - by,
            x1: self.x1 + by,
            z0: self.z0 - by,
            z1: self.z1 + by,
        }
    }
}

/// How far a chunk has been generated. The holders are all published at
/// `ChunkStatus::Terrain` up front (a published status cannot be raised later,
/// and the feature region accepts any dependency status up to Terrain), so the
/// real progress is tracked here.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Stage {
    Empty,
    Starts,
    References,
    Biomes,
    Terrain,
    Features,
}

struct Pipeline<'a> {
    generator: &'a Arc<ChunkGeneratorType>,
    context: &'a Arc<WorldGenContext>,
    feature_step: &'a ChunkStep,
    feature_cache_radius: i32,
    min_y: i32,
    height: i32,
    section_count: usize,
    holders: HolderMap,
    stages: FxHashMap<(i32, i32), Stage>,
}

impl Pipeline<'_> {
    fn chunk(&self, pos: (i32, i32)) -> &Chunk {
        let Some(holder) = self.holders.get(&pos) else {
            panic!("no chunk prepared at ({}, {})", pos.0, pos.1);
        };
        let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) else {
            panic!("chunk ({}, {}) was never installed", pos.0, pos.1);
        };
        chunk
    }

    /// Creates empty chunks for every position of the rectangle that has none yet.
    fn ensure(&mut self, rect: Rect) {
        for x in rect.x0..=rect.x1 {
            for z in rect.z0..=rect.z1 {
                self.holders.entry((x, z)).or_insert_with(|| {
                    let holder = Arc::new(ChunkHolder::new(
                        ChunkPos::new(x, z),
                        ChunkTicketLevel::STRONGEST,
                        None,
                        self.min_y,
                        self.height,
                    ));
                    holder.insert_chunk(
                        empty_proto_chunk((x, z), self.section_count, self.min_y, self.height),
                        ChunkStatus::Terrain,
                    );
                    holder
                });
                self.stages.entry((x, z)).or_insert(Stage::Empty);
            }
        }
    }

    /// Chunks of the rectangle that have not reached `stage`, in x-then-z order.
    fn pending(&self, rect: Rect, stage: Stage) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for x in rect.x0..=rect.x1 {
            for z in rect.z0..=rect.z1 {
                let Some(&current) = self.stages.get(&(x, z)) else {
                    panic!("no chunk prepared at ({x}, {z})");
                };
                if current < stage {
                    out.push((x, z));
                }
            }
        }
        out
    }

    fn mark(&mut self, targets: &[(i32, i32)], stage: Stage) {
        for &pos in targets {
            self.stages.insert(pos, stage);
        }
    }

    /// `STRUCTURE_STARTS`. Sequential like vanilla's consecutive worldgen executor.
    fn run_starts(&self, targets: &[(i32, i32)]) {
        for &pos in targets {
            self.generator.create_structures(self.chunk(pos));
        }
    }

    /// `STRUCTURE_REFERENCES`, the mirror of `worldgen::stages::structures::generate_references`:
    /// record which chunks within 8 hold a start whose inflated box reaches the target.
    fn run_references(&self, targets: &[(i32, i32)]) {
        for &(target_x, target_z) in targets {
            let target_block_x = target_x * 16;
            let target_block_z = target_z * 16;
            let target = self.chunk((target_x, target_z));
            for source_x in (target_x - DEPENDENCY_RADIUS)..=(target_x + DEPENDENCY_RADIUS) {
                for source_z in (target_z - DEPENDENCY_RADIUS)..=(target_z + DEPENDENCY_RADIUS) {
                    let Some(source_holder) = self.holders.get(&(source_x, source_z)) else {
                        continue;
                    };
                    let Some(source) = source_holder.try_chunk(ChunkStatus::Empty) else {
                        continue;
                    };
                    let starts = source.structure_starts();
                    for (structure_id, start) in starts.iter() {
                        let Some(bb) = start.bounding_box else {
                            continue;
                        };
                        if bb.intersects_xz(
                            target_block_x,
                            target_block_z,
                            target_block_x + 15,
                            target_block_z + 15,
                        ) {
                            target
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

    /// BIOMES, in parallel: each chunk only reads the biome source.
    fn run_biomes(&self, targets: &[(i32, i32)]) {
        targets
            .par_iter()
            .for_each(|&pos| self.generator.create_biomes(self.chunk(pos)));
    }

    /// Beardifier from the chunk's structure references, like
    /// `worldgen::stages::terrain::build_beardifier` but reading this map.
    fn beardifier(&self, chunk: &Chunk) -> Option<Beardifier> {
        let references = chunk.structure_references();
        let mut source_positions: FxHashSet<ChunkPos> = FxHashSet::default();
        for source_chunks in references.values() {
            source_positions.extend(source_chunks.iter().copied());
        }
        if source_positions.is_empty() {
            return None;
        }

        let mut source_indices: FxHashMap<ChunkPos, usize> = FxHashMap::default();
        let mut starts_guards = Vec::with_capacity(source_positions.len());
        for &source_pos in &source_positions {
            let source = self.chunk((source_pos.0.x, source_pos.0.y));
            source_indices.insert(source_pos, starts_guards.len());
            starts_guards.push(source.structure_starts());
        }

        let mut starts: Vec<&StructureStart> = Vec::new();
        for (structure_id, source_chunks) in references.iter() {
            for &source_pos in source_chunks {
                let Some(&guard_index) = source_indices.get(&source_pos) else {
                    continue;
                };
                if let Some(start) = starts_guards[guard_index].get(structure_id)
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

        let beardifier = Beardifier::for_structures_in_chunk(
            starts.iter().copied(),
            chunk.pos.0.x,
            chunk.pos.0.y,
        );
        (!beardifier.is_empty()).then_some(beardifier)
    }

    /// TERRAIN, in parallel: every chunk writes only its own sections and reads
    /// neighbor biomes and starts behind their locks.
    fn run_terrain(&self, targets: &[(i32, i32)]) {
        let min_quart_y = self.min_y >> 2;
        let total_quarts_y = (self.section_count * 4) as i32;
        targets.par_iter().for_each(|&pos| {
            let chunk = self.chunk(pos);
            let beardifier = self.beardifier(chunk);
            let neighbor_biomes = |quart_pos: IVec3| -> u16 {
                let neighbor_chunk_x = quart_pos.x >> 2;
                let neighbor_chunk_z = quart_pos.z >> 2;
                let neighbor = self.chunk((neighbor_chunk_x, neighbor_chunk_z));
                let local_quart_x = (quart_pos.x - neighbor_chunk_x * 4) as usize;
                let local_quart_z = (quart_pos.z - neighbor_chunk_z * 4) as usize;
                let local_quart_y =
                    (quart_pos.y - min_quart_y).clamp(0, total_quarts_y - 1) as usize;
                let section_index = local_quart_y / 4;
                neighbor.sections.sections[section_index].read().biomes.get(
                    local_quart_x,
                    local_quart_y % 4,
                    local_quart_z,
                )
            };
            self.generator.build_terrain(
                GenerationChunk::<TerrainPhase>::for_test(chunk),
                beardifier.as_ref(),
                &neighbor_biomes,
            );
        });
    }

    /// FEATURES for the rectangle in global x-then-z order, one chunk at a time.
    fn run_features(&self, snapshot: &Arc<HolderMap>, rect: Rect) {
        for x in rect.x0..=rect.x1 {
            for z in rect.z0..=rect.z1 {
                let center = ChunkPos::new(x, z);
                // Like `worldgen::stages::features::generate`: final heightmaps are primed
                // right before the chunk's own features.
                self.chunk((x, z)).prime_final_heightmaps();
                let cache_holders = Arc::clone(snapshot);
                let cache = Arc::new(StaticCache2D::create(
                    x,
                    z,
                    self.feature_cache_radius,
                    move |cx, cz| match cache_holders.get(&(cx, cz)) {
                        Some(holder) => Arc::clone(holder),
                        None => panic!("missing feature dependency chunk ({cx}, {cz})"),
                    },
                ));
                let random = self
                    .generator
                    .create_worldgen_region_random(seed() as i64, center);
                let mut region =
                    WorldGenRegion::new(self.context, self.feature_step, &cache, center, random);
                self.generator.apply_biome_decorations(&mut region);
            }
        }
    }

    /// Drops every chunk left of `min_x`; later bands never look that far back.
    fn evict_before(&mut self, min_x: i32) {
        self.holders.retain(|&(x, _), _| x >= min_x);
        self.stages.retain(|&(x, _), _| x >= min_x);
    }
}

struct Mismatch {
    pos: (i32, i32),
    expected: String,
    actual: String,
    /// Block-level report when a debug dump covers the chunk.
    detail: Option<String>,
}

#[derive(Default)]
struct StageReport {
    compared: usize,
    /// Mismatches matching a [`KnownDivergence`] entry exactly.
    known: usize,
    mismatches: Vec<Mismatch>,
}

impl StageReport {
    fn push(&mut self, dim_short: &str, stage: &str, mismatch: Mismatch) {
        if let Some(known) = KnownDivergence::find(dim_short, stage, mismatch.pos)
            && known.expected == mismatch.expected
        {
            eprintln!(
                "[{dim_short}/{stage}] ({:4},{:4}) known divergence ({}), actual={}",
                mismatch.pos.0, mismatch.pos.1, known.reason, mismatch.actual
            );
            self.known += 1;
            return;
        }
        eprintln!(
            "[{dim_short}/{stage}] ({:4},{:4}) MISMATCH expected={} actual={}",
            mismatch.pos.0, mismatch.pos.1, mismatch.expected, mismatch.actual
        );
        self.mismatches.push(mismatch);
    }

    /// Flags a known-divergence entry whose chunk now matches vanilla, so it can be removed.
    fn note_match(dim_short: &str, stage: &str, pos: (i32, i32)) {
        if KnownDivergence::find(dim_short, stage, pos).is_some() {
            eprintln!(
                "[{dim_short}/{stage}] ({:4},{:4}) matches vanilla but is listed in KNOWN_DIVERGENCES; remove the entry",
                pos.0, pos.1
            );
        }
    }

    fn append_report(&self, msg: &mut String, dim_short: &str, stage: &str) {
        if self.mismatches.is_empty() {
            return;
        }
        let _ = writeln!(
            msg,
            "{dim_short}/{stage}: {}/{} chunks do not match vanilla",
            self.mismatches.len(),
            self.compared
        );
        // Every chunk the debug dump explains is shown; the rest is capped.
        for mismatch in &self.mismatches {
            if let Some(detail) = &mismatch.detail {
                msg.push_str(detail);
            }
        }
        let undetailed = self
            .mismatches
            .iter()
            .filter(|mismatch| mismatch.detail.is_none())
            .collect::<Vec<_>>();
        for mismatch in undetailed.iter().take(MAX_REPORTED_MISMATCHES) {
            let _ = writeln!(
                msg,
                "  ({:4},{:4}): expected {}, got {}",
                mismatch.pos.0, mismatch.pos.1, mismatch.expected, mismatch.actual
            );
        }
        if undetailed.len() > MAX_REPORTED_MISMATCHES {
            let _ = writeln!(
                msg,
                "  ... and {} more without a debug dump",
                undetailed.len() - MAX_REPORTED_MISMATCHES
            );
        }
    }
}

/// Compares a chunk's block hash with the fixture, attaching a block diff when a
/// debug dump covers it.
#[expect(
    clippy::too_many_arguments,
    reason = "the stage, fixture and debug dump are independent inputs of one comparison"
)]
fn check_hash(
    report: &mut StageReport,
    dim_short: &str,
    stage: &str,
    pos: (i32, i32),
    chunk: &Chunk,
    fixture: &ChunkHashes,
    debug_blocks: Option<&FxHashMap<(i32, i32), ChunkBlockData>>,
    min_y: i32,
) {
    let Some(&expected) = fixture.get(&pos) else {
        panic!(
            "chunk ({}, {}) is missing from the {dim_short}/{stage} fixture",
            pos.0, pos.1
        );
    };
    report.compared += 1;
    let actual = compute_block_hash(&chunk.sections);
    if actual.0 == expected {
        StageReport::note_match(dim_short, stage, pos);
        return;
    }
    let detail = debug_blocks
        .and_then(|blocks| blocks.get(&pos))
        .map(|reference| diff_chunk(&chunk.sections, reference, min_y))
        .filter(|diffs| !diffs.is_empty())
        .map(|diffs| format_chunk_diffs(&diffs, pos.0, pos.1, min_y));
    report.push(
        dim_short,
        stage,
        Mismatch {
            pos,
            expected: format!("{:x}", md5::Digest(expected)),
            actual: format!("{actual:x}"),
            detail,
        },
    );
}

#[test]
#[ignore = "generates up to one million chunks per dimension with features; run with --release"]
fn large_scale_feature_hashes() {
    use std::panic;
    use std::thread;

    // Structure assembly and feature placement recurse deeply in debug builds.
    let result = thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(large_scale_feature_hashes_inner)
        .expect("Failed to spawn test thread")
        .join();

    if let Err(payload) = result {
        panic::resume_unwind(payload);
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one pass per dimension: fixture load, generator setup, band sweep, report"
)]
fn large_scale_feature_hashes_inner() {
    use crate::bootstrap::init_globals;

    init_globals();

    let feature_step = GENERATION_PYRAMID.get_step_to(ChunkStatus::Features);
    let feature_cache_radius = feature_step.direct_dependencies.get_radius() as i32;
    assert!(
        feature_cache_radius <= DEPENDENCY_RADIUS,
        "feature step needs radius {feature_cache_radius}, the sweep only prepares {DEPENDENCY_RADIUS}"
    );
    let band_width = parse_env_i32(BAND_ENV).unwrap_or(DEFAULT_BAND_WIDTH);
    let restricted_half = parse_env_i32(HALF_SIZE_ENV);
    let stop_x = parse_env_any_i32(STOP_X_ENV);
    let debug_dimension = debug_dimension_filter();
    let stop_after_first_mismatch = env::var_os(STOP_AFTER_FIRST_MISMATCH_ENV).is_some();

    // Biomes and terrain of a band fan out on this pool; starts, references and
    // features stay sequential.
    let pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .stack_size(64 * 1024 * 1024)
            .thread_name(|index| format!("large-scale-features-{index}"))
            .build()
            .expect("failed to create large-scale feature hash test pool"),
    );
    // The generator only uses its pool while constructing biome and structure data.
    // Like the other hash tests, keep that single-threaded so the possible-biome
    // order, and with it the per-step feature order, is deterministic.
    let generator_pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .stack_size(64 * 1024 * 1024)
            .thread_name(|index| format!("large-scale-features-init-{index}"))
            .build()
            .expect("failed to create large-scale feature hash test init pool"),
    );
    let mut checked_dimensions = 0usize;

    for &dim_key in DIMENSION_ORDER {
        if debug_dimension
            .as_deref()
            .is_some_and(|filter| filter != dim_key)
        {
            continue;
        }
        let dim_short = dim_key.strip_prefix("minecraft:").unwrap_or(dim_key);
        let Some(features_fixture) = load_hashes(dim_short, FEATURES_STAGE) else {
            eprintln!("[{dim_short}] no large-scale features fixture, skipping");
            continue;
        };
        let terrain_fixture = load_hashes(dim_short, TERRAIN_STAGE);
        let starts_fixture = load_starts(dim_short);
        let debug_terrain_blocks = read_reference_blocks(&format!(
            "{}/test_assets/large_scale_{dim_short}_debug_{TERRAIN_STAGE}_blocks{}.bin.gz",
            env!("CARGO_MANIFEST_DIR"),
            seed_suffix()
        ));
        let debug_feature_blocks = read_reference_blocks(&format!(
            "{}/test_assets/large_scale_{dim_short}_debug_{FEATURES_STAGE}_blocks{}.bin.gz",
            env!("CARGO_MANIFEST_DIR"),
            seed_suffix()
        ));
        checked_dimensions += 1;

        let half = square_half_size(&features_fixture, dim_short, FEATURES_STAGE);
        if let Some(terrain) = &terrain_fixture {
            assert_eq!(
                square_half_size(terrain, dim_short, TERRAIN_STAGE),
                half,
                "[{dim_short}] terrain and features fixtures cover different squares"
            );
        }
        let sweep_half = restricted_half.map_or(half, |restricted| restricted.min(half));
        let margin = if sweep_half < half {
            RESTRICTED_MARGIN
        } else {
            0
        };
        let comparable = |(x, z): (i32, i32)| {
            x >= -sweep_half + margin
                && x < sweep_half - margin
                && z >= -sweep_half + margin
                && z < sweep_half - margin
        };
        let inside_square = |(x, z): (i32, i32)| {
            x >= -sweep_half && x < sweep_half && z >= -sweep_half && z < sweep_half
        };
        let total_chunks = u64::try_from(sweep_half).unwrap_or(0).pow(2) * 4;
        eprintln!(
            "[{dim_short}] sweeping [-{sweep_half}, {sweep_half}) in bands of {band_width} \
             (fixture [-{half}, {half}), seed {}, terrain fixture: {}, starts fixture: {})",
            seed() as i64,
            terrain_fixture.is_some(),
            starts_fixture.is_some()
        );

        let dim_type = match dim_key {
            "minecraft:overworld" => &vanilla_dimension_types::OVERWORLD,
            "minecraft:the_nether" => &vanilla_dimension_types::THE_NETHER,
            "minecraft:the_end" => &vanilla_dimension_types::THE_END,
            _ => panic!("Unknown dimension: {dim_key}"),
        };
        let generator: Arc<ChunkGeneratorType> = Arc::new(match dim_key {
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
        });
        let damage_history = Arc::new(DamageHistory::default());
        let world = create_test_world(
            dim_key,
            dim_type,
            seed(),
            Arc::clone(&generator),
            Arc::clone(&generator_pool),
            &damage_history,
        );
        let context = Arc::clone(&world.chunk_map.world_gen_context);

        let mut pipeline = Pipeline {
            generator: &generator,
            context: &context,
            feature_step,
            feature_cache_radius,
            min_y: dim_type.min_y,
            height: dim_type.height,
            section_count: (dim_type.height / 16) as usize,
            holders: HolderMap::default(),
            stages: FxHashMap::default(),
        };

        let mut terrain_report = StageReport::default();
        let mut features_report = StageReport::default();
        let mut starts_report = StageReport::default();
        let started = Instant::now();
        let mut featured = 0u64;
        let mut finalized = 0u64;
        let z_min = -sweep_half;
        let z_max = sweep_half - 1;
        let mut bx0 = -sweep_half;

        while bx0 < sweep_half {
            let bx1 = (bx0 + band_width).min(sweep_half); // exclusive
            let first = bx0 == -sweep_half;
            let last = bx1 == sweep_half;
            let band_started = Instant::now();

            // Features need terrain one chunk out, terrain needs biomes one further,
            // and everything needs structure starts DEPENDENCY_RADIUS further.
            let feature_rect = Rect {
                x0: bx0,
                x1: bx1 - 1,
                z0: z_min,
                z1: z_max,
            };
            let terrain_rect = feature_rect.grow(1);
            let biome_rect = terrain_rect.grow(1);
            let starts_rect = biome_rect.grow(DEPENDENCY_RADIUS);

            pipeline.ensure(starts_rect);

            let starts_targets = pipeline.pending(starts_rect, Stage::Starts);
            pipeline.run_starts(&starts_targets);
            pipeline.mark(&starts_targets, Stage::Starts);
            let starts_done = Instant::now();
            if let Some(fixture) = &starts_fixture {
                for &pos in starts_targets.iter().filter(|&&pos| inside_square(pos)) {
                    let mut actual: Vec<String> = pipeline
                        .chunk(pos)
                        .structure_starts()
                        .keys()
                        .map(ToString::to_string)
                        .collect();
                    actual.sort_unstable();
                    let expected = fixture.get(&pos).cloned().unwrap_or_default();
                    starts_report.compared += 1;
                    if actual == expected {
                        StageReport::note_match(dim_short, STARTS_STAGE, pos);
                    } else {
                        starts_report.push(
                            dim_short,
                            STARTS_STAGE,
                            Mismatch {
                                pos,
                                expected: if expected.is_empty() {
                                    "none".to_owned()
                                } else {
                                    expected.join("+")
                                },
                                actual: if actual.is_empty() {
                                    "none".to_owned()
                                } else {
                                    actual.join("+")
                                },
                                detail: None,
                            },
                        );
                    }
                }
            }

            let reference_targets = pipeline.pending(biome_rect, Stage::References);
            pipeline.run_references(&reference_targets);
            pipeline.mark(&reference_targets, Stage::References);
            let biome_targets = pipeline.pending(biome_rect, Stage::Biomes);
            pool.install(|| pipeline.run_biomes(&biome_targets));
            pipeline.mark(&biome_targets, Stage::Biomes);
            let biomes_done = Instant::now();

            let terrain_targets = pipeline.pending(terrain_rect, Stage::Terrain);
            pool.install(|| pipeline.run_terrain(&terrain_targets));
            pipeline.mark(&terrain_targets, Stage::Terrain);
            let terrain_done = Instant::now();
            // Terrain hashes are taken now, before any neighbor's features can write
            // into these chunks.
            if let Some(fixture) = &terrain_fixture {
                for &pos in terrain_targets.iter().filter(|&&pos| inside_square(pos)) {
                    check_hash(
                        &mut terrain_report,
                        dim_short,
                        TERRAIN_STAGE,
                        pos,
                        pipeline.chunk(pos),
                        fixture,
                        debug_terrain_blocks.as_ref(),
                        dim_type.min_y,
                    );
                }
            }

            let snapshot = Arc::new(pipeline.holders.clone());
            pipeline.run_features(&snapshot, feature_rect);
            drop(snapshot);
            let feature_targets = pipeline.pending(feature_rect, Stage::Features);
            pipeline.mark(&feature_targets, Stage::Features);
            featured += feature_targets.len() as u64;
            let features_done = Instant::now();

            // A chunk is final once its right-hand neighbors have run features too, so
            // the band's last column waits for the next band unless this is the last one.
            let final_x0 = if first { bx0 } else { bx0 - 1 };
            let final_x1 = if last { bx1 - 1 } else { bx1 - 2 };
            for x in final_x0..=final_x1 {
                for z in z_min..=z_max {
                    finalized += 1;
                    if !comparable((x, z)) {
                        continue;
                    }
                    check_hash(
                        &mut features_report,
                        dim_short,
                        FEATURES_STAGE,
                        (x, z),
                        pipeline.chunk((x, z)),
                        &features_fixture,
                        debug_feature_blocks.as_ref(),
                        dim_type.min_y,
                    );
                }
            }

            // The next band needs starts out to DEPENDENCY_RADIUS left of its biome ring.
            pipeline.evict_before(bx1 - 2 - DEPENDENCY_RADIUS);

            let elapsed = started.elapsed().as_secs_f64();
            let rate = featured as f64 / elapsed;
            let remaining = (total_chunks - featured) as f64 / rate;
            eprintln!(
                "[{dim_short}] band [{bx0},{bx1}) done, {featured}/{total_chunks} chunks featured, \
                 {finalized} final ({rate:.1}/s, {elapsed:.0}s elapsed, ~{remaining:.0}s remaining; \
                 starts {:.1}s, refs+biomes {:.1}s, terrain {:.1}s, features {:.1}s; \
                 mismatches: starts {}, terrain {}, features {}; {} chunks live)",
                (starts_done - band_started).as_secs_f64(),
                (biomes_done - starts_done).as_secs_f64(),
                (terrain_done - biomes_done).as_secs_f64(),
                (features_done - terrain_done).as_secs_f64(),
                starts_report.mismatches.len(),
                terrain_report.mismatches.len(),
                features_report.mismatches.len(),
                pipeline.holders.len(),
            );

            let any_mismatch = !starts_report.mismatches.is_empty()
                || !terrain_report.mismatches.is_empty()
                || !features_report.mismatches.is_empty();
            if stop_after_first_mismatch && any_mismatch {
                eprintln!("[{dim_short}] stopping after the first band with mismatches");
                break;
            }
            if stop_x.is_some_and(|stop| final_x1 >= stop) {
                eprintln!("[{dim_short}] column {final_x1} is final, stopping at {STOP_X_ENV}");
                break;
            }
            bx0 = bx1;
        }

        let mut msg = String::new();
        starts_report.append_report(&mut msg, dim_short, STARTS_STAGE);
        terrain_report.append_report(&mut msg, dim_short, TERRAIN_STAGE);
        features_report.append_report(&mut msg, dim_short, FEATURES_STAGE);
        assert!(msg.is_empty(), "{msg}");
        eprintln!(
            "[{dim_short}] OK: {} starts, {} terrain and {} feature chunks match vanilla \
             ({} known divergences skipped)",
            starts_report.compared,
            terrain_report.compared,
            features_report.compared,
            starts_report.known + terrain_report.known + features_report.known
        );
    }

    assert!(
        checked_dimensions > 0,
        "no large-scale features fixtures found; produce them with SteelExtractor's \
         STEEL_EXTRACTOR_LARGE_SCALE_FEATURES=1 mode and copy them into test_assets"
    );
}
