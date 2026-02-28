//! Biome regression tests.
//!
//! Verifies that Steel's biome generation matches vanilla Minecraft
//! by comparing per-chunk MD5 hashes of biome names across all sections.
//!
//! Hashes are loaded from `biome_hashes.json`, extracted from vanilla using the Extractor mod.

use std::fmt::Write;
use std::thread;

use rustc_hash::FxHashMap;
use serde::Deserialize;
use steel_core::worldgen::{BiomeSourceKind, ChunkBiomeSampler};
use steel_registry::biome::BiomeRef;

/// Top-level JSON structure for biome hashes.
#[derive(Deserialize)]
struct BiomeHashesJson {
    seed: u64,
    overworld: DimensionHashes,
    the_nether: DimensionHashes,
    the_end: DimensionHashes,
}

/// Per-dimension hash data.
#[derive(Deserialize)]
struct DimensionHashes {
    min_section_y: i32,
    max_section_y: i32,
    hashes: Vec<(i32, i32, String)>,
}

fn load_expected_hashes() -> BiomeHashesJson {
    let json_str = include_str!("../test_assets/biome_hashes.json");
    serde_json::from_str(json_str).expect("Failed to parse biome_hashes.json")
}

/// Compute a biome MD5 hash for a chunk using a [`ChunkBiomeSampler`].
///
/// Samples biomes in vanilla's generation iteration order (section → X → Y → Z),
/// then hashes in deterministic Y → Z → X order with `section_y` markers.
fn chunk_biome_hash(
    sampler: &mut ChunkBiomeSampler<'_>,
    chunk_x: i32,
    chunk_z: i32,
    min_section_y: i32,
    max_section_y: i32,
) -> String {
    let mut biomes: FxHashMap<(i32, i32, i32, i32), BiomeRef> = FxHashMap::default();

    for section_y in min_section_y..=max_section_y {
        for x in 0..4i32 {
            for y in 0..4i32 {
                for z in 0..4i32 {
                    let quart_x = chunk_x * 4 + x;
                    let quart_y = section_y * 4 + y;
                    let quart_z = chunk_z * 4 + z;

                    let biome = sampler.sample(quart_x, quart_y, quart_z);
                    biomes.insert((section_y, x, y, z), biome);
                }
            }
        }
    }

    let mut ctx = md5::Context::new();
    for section_y in min_section_y..=max_section_y {
        ctx.consume([section_y as u8]);
        for y in 0..4i32 {
            for z in 0..4i32 {
                for x in 0..4i32 {
                    let biome = biomes[&(section_y, x, y, z)];
                    ctx.consume(biome.key.path.as_bytes());
                }
            }
        }
    }

    format!("{:x}", ctx.finalize())
}

/// Verify biome hashes for a dimension using a [`BiomeSourceKind`].
fn verify_dimension(source: &BiomeSourceKind, dim: &DimensionHashes, dimension_name: &str) {
    let mut mismatches = Vec::new();

    for (chunk_x, chunk_z, expected_hash) in &dim.hashes {
        let mut sampler = source.chunk_sampler();
        let actual_hash = chunk_biome_hash(
            &mut sampler,
            *chunk_x,
            *chunk_z,
            dim.min_section_y,
            dim.max_section_y,
        );
        if actual_hash != *expected_hash {
            mismatches.push((*chunk_x, *chunk_z, expected_hash.clone(), actual_hash));
        }
    }

    if !mismatches.is_empty() {
        let total = dim.hashes.len();
        let failed = mismatches.len();
        let mut msg = format!("{dimension_name}: {failed}/{total} chunks MISMATCHED:\n");
        for (x, z, expected, actual) in &mismatches {
            let _ = writeln!(msg, "  ({x:3},{z:3}): expected {expected} got {actual}");
        }
        panic!("{msg}");
    }
}

#[test]
fn overworld_biome_hashes_match_vanilla() {
    let expected = load_expected_hashes();

    // Climate sampler initialization has deep recursion; needs a large stack.
    let builder = thread::Builder::new().stack_size(16 * 1024 * 1024);
    let handle = builder
        .spawn(move || {
            let source = BiomeSourceKind::overworld(expected.seed);
            verify_dimension(&source, &expected.overworld, "overworld");
        })
        .expect("failed to spawn test thread");

    handle.join().expect("test thread panicked");
}

#[test]
fn nether_biome_hashes_match_vanilla() {
    let expected = load_expected_hashes();

    let builder = thread::Builder::new().stack_size(16 * 1024 * 1024);
    let handle = builder
        .spawn(move || {
            let source = BiomeSourceKind::nether(expected.seed);
            verify_dimension(&source, &expected.the_nether, "the_nether");
        })
        .expect("failed to spawn test thread");

    handle.join().expect("test thread panicked");
}

#[test]
fn end_biome_hashes_match_vanilla() {
    let expected = load_expected_hashes();

    let source = BiomeSourceKind::end(expected.seed);
    verify_dimension(&source, &expected.the_end, "the_end");
}
