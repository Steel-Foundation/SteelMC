//! Biome regression test.
//!
//! Verifies that Steel's biome generation matches vanilla Minecraft
//! by comparing per-chunk MD5 hashes of biome names across all 24 sections.
//!
//! Hashes are loaded from `biome_hashes.json`, extracted from vanilla using the Extractor mod.

use std::fmt::Write;
use std::thread;

use rustc_hash::FxHashMap;
use serde::Deserialize;
use steel_core::worldgen::{OverworldColumnCache, VanillaClimateSampler};
use steel_registry::multi_noise::get_overworld_biome_cached;

/// JSON structure for biome hashes
#[derive(Deserialize)]
struct BiomeHashesJson {
    seed: u64,
    #[allow(dead_code)]
    radius: i32,
    hashes: Vec<(i32, i32, String)>,
}

/// Load expected hashes from JSON
fn load_expected_hashes() -> BiomeHashesJson {
    let json_str = include_str!("../test_assets/biome_hashes.json");
    serde_json::from_str(json_str).expect("Failed to parse biome_hashes.json")
}

/// Compute a biome MD5 hash for a chunk.
///
/// Samples biomes using vanilla's generation iteration order (X,Y,Z) with cache
/// so that tie-breaking matches actual world generation, then hashes in
/// deterministic Y,Z,X order with `section_y` markers for a stable digest.
fn chunk_biome_hash(sampler: &VanillaClimateSampler, chunk_x: i32, chunk_z: i32) -> String {
    // Step 1: Sample biomes in generation order (X outer, Y middle, Z inner)
    // so the cache tie-breaking matches vanilla/Steel world generation.
    let mut biomes = FxHashMap::default();
    let mut biome_cache: Option<usize> = None;
    let mut eval_cache = OverworldColumnCache::new();

    for section_y in -4i32..20 {
        for x in 0..4i32 {
            for y in 0..4i32 {
                for z in 0..4i32 {
                    let quart_x = chunk_x * 4 + x;
                    let quart_y = section_y * 4 + y;
                    let quart_z = chunk_z * 4 + z;

                    let target = sampler.sample(quart_x, quart_y, quart_z, &mut eval_cache);
                    let biome = get_overworld_biome_cached(&target, &mut biome_cache);
                    biomes.insert((section_y, x, y, z), &biome.key);
                }
            }
        }
    }

    // Step 2: Hash in deterministic Y,Z,X order with section markers.
    let mut ctx = md5::Context::new();
    for section_y in -4i32..20 {
        ctx.consume([section_y as u8]);
        for y in 0..4i32 {
            for z in 0..4i32 {
                for x in 0..4i32 {
                    let key = biomes[&(section_y, x, y, z)];
                    ctx.consume(key.path.as_bytes());
                }
            }
        }
    }

    format!("{:x}", ctx.finalize())
}

#[test]
fn biome_hashes_match_vanilla() {
    let expected = load_expected_hashes();

    // VanillaClimateSampler initialization has deep recursion; needs a large stack.
    let builder = thread::Builder::new().stack_size(16 * 1024 * 1024);
    let handle = builder
        .spawn(move || {
            let sampler = VanillaClimateSampler::new(expected.seed);

            let mut mismatches = Vec::new();

            for (chunk_x, chunk_z, expected_hash) in &expected.hashes {
                let actual_hash = chunk_biome_hash(&sampler, *chunk_x, *chunk_z);
                if actual_hash != *expected_hash {
                    mismatches.push((*chunk_x, *chunk_z, expected_hash.clone(), actual_hash));
                }
            }

            if !mismatches.is_empty() {
                let total = expected.hashes.len();
                let failed = mismatches.len();
                let mut msg = format!("{failed}/{total} chunks MISMATCHED:\n");
                for (x, z, expected, actual) in &mismatches {
                    let _ = writeln!(msg, "  ({x:3},{z:3}): expected {expected} got {actual}");
                }
                panic!("{msg}");
            }
        })
        .expect("failed to spawn test thread");

    handle.join().expect("test thread panicked");
}
