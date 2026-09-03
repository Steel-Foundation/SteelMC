//! Structure start fixture test.
//!
//! Compares Steel's generated `minecraft:trial_chambers` structure starts
//! against vanilla reference data extracted by SteelExtractor
//! (`test_assets/structure_starts.json`, seed 13579). A piece-for-piece match
//! proves that placement selection, jigsaw assembly, and pool aliases are
//! deterministic and vanilla-exact for trial chambers.

use std::sync::Arc;

use serde::Deserialize;

use crate::chunk::Chunk;
use crate::worldgen::{ChunkGeneratorType, OverworldGenerator};
use steel_registry::structure::template_pool::PoolElement;
use steel_utils::ChunkPos;
use steel_worldgen::biomes::BiomeSourceKind;
use steel_worldgen::structure::StructurePiecePayload;

#[derive(Clone, Deserialize, Debug)]
struct FixtureBoundingBox {
    min_x: i32,
    min_y: i32,
    min_z: i32,
    max_x: i32,
    max_y: i32,
    max_z: i32,
}

#[derive(Clone, Deserialize, Debug)]
struct FixturePiece {
    bounding_box: FixtureBoundingBox,
    #[serde(default)]
    piece_data: Option<FixturePieceData>,
}

#[derive(Clone, Deserialize, Debug)]
struct FixturePieceData {
    #[serde(default)]
    pool_element: Option<FixturePoolElement>,
}

#[derive(Clone, Deserialize, Debug)]
struct FixturePoolElement {
    #[serde(default)]
    location: Option<String>,
}

#[derive(Clone, Deserialize, Debug)]
struct FixtureStart {
    structure: String,
    pieces: Vec<FixturePiece>,
}

#[derive(Clone, Deserialize, Debug)]
struct FixtureChunk {
    x: i32,
    z: i32,
    #[serde(default)]
    starts: Vec<FixtureStart>,
}

#[derive(Clone, Deserialize, Debug)]
struct FixtureRoot {
    seed: u64,
    overworld: FixtureOverworld,
}

#[derive(Clone, Deserialize, Debug)]
struct FixtureOverworld {
    chunks: Vec<FixtureChunk>,
}

fn load_trial_chambers_fixtures() -> Vec<(i32, i32, FixtureStart)> {
    let root: FixtureRoot =
        serde_json::from_str(include_str!("../../test_assets/structure_starts.json"))
            .expect("valid structure_starts.json fixture");
    assert_eq!(root.seed, 13579, "fixture seed changed");

    let mut fixtures = Vec::new();
    for chunk in &root.overworld.chunks {
        for start in &chunk.starts {
            if start.structure == "minecraft:trial_chambers" {
                fixtures.push((chunk.x, chunk.z, start.clone()));
            }
        }
    }
    fixtures
}

fn overworld_generator(seed: u64, pool: &Arc<rayon::ThreadPool>) -> ChunkGeneratorType {
    let source = BiomeSourceKind::overworld(seed);
    ChunkGeneratorType::Overworld(OverworldGenerator::new(None, source, seed, pool))
}

fn proto_chunk(pos: (i32, i32)) -> Chunk {
    let dim_type = &steel_registry::vanilla_dimension_types::OVERWORLD;
    let min_y = dim_type.min_y;
    let height = dim_type.height;
    let section_count = (height / 16) as usize;
    let sections: Box<[crate::chunk::section::ChunkSection]> = (0..section_count)
        .map(|_| crate::chunk::section::ChunkSection::new_empty())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    Chunk::new(
        crate::chunk::section::Sections::from_owned(sections),
        ChunkPos::new(pos.0, pos.1),
        min_y,
        height,
        std::sync::Weak::new(),
    )
}

/// A piece matches when its bounding box and (for template pieces) its pool
/// template location equal the vanilla fixture values.
fn piece_matches(piece: &steel_worldgen::structure::StructurePiece, fixture: &FixturePiece) -> bool {
    use steel_utils::axis::Axis;
    let bb = &piece.bounding_box;
    let bounds_match = bb.min(Axis::X) == fixture.bounding_box.min_x
        && bb.min(Axis::Y) == fixture.bounding_box.min_y
        && bb.min(Axis::Z) == fixture.bounding_box.min_z
        && bb.max(Axis::X) == fixture.bounding_box.max_x
        && bb.max(Axis::Y) == fixture.bounding_box.max_y
        && bb.max(Axis::Z) == fixture.bounding_box.max_z;
    if !bounds_match {
        return false;
    }
    let Some(fixture_piece_data) = &fixture.piece_data else {
        return false;
    };
    match (&piece.payload, &fixture_piece_data.pool_element) {
        (
            StructurePiecePayload::Jigsaw(data),
            Some(FixturePoolElement {
                location: Some(location),
            }),
        ) => match &data.pool_element {
            PoolElement::Single {
                location: steel_location,
                ..
            }
            | PoolElement::LegacySingle {
                location: steel_location,
                ..
            } => &steel_location.to_string() == location,
            _ => false,
        },
        _ => true,
    }
}

#[test]
fn trial_chambers_starts_match_vanilla_fixture_pieces() {
    use crate::bootstrap::init_globals_once;
    init_globals_once();

    let fixtures = load_trial_chambers_fixtures();
    assert!(
        fixtures.len() >= 3,
        "expected several trial chamber fixtures, found {}",
        fixtures.len()
    );

    let pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("test rayon pool"),
    );
    let generator = overworld_generator(13579, &pool);

    for (chunk_x, chunk_z, fixture) in &fixtures {
        let chunk = proto_chunk((*chunk_x, *chunk_z));
        crate::worldgen::ChunkGenerator::create_structures(&generator, &chunk);

        let starts = chunk.structure_starts();
        let start = starts
            .iter()
            .find(|(id, _)| id.path == "trial_chambers")
            .map(|(_, start)| start)
            .unwrap_or_else(|| {
                panic!(
                    "trial chambers start missing at chunk ({chunk_x}, {chunk_z})"
                )
            });

        assert_eq!(
            start.pieces.len(),
            fixture.pieces.len(),
            "piece count mismatch at ({chunk_x}, {chunk_z})"
        );
        for fixture_piece in &fixture.pieces {
            assert!(
                start.pieces.iter().any(|piece| piece_matches(piece, fixture_piece)),
                "missing vanilla piece with bbox {:?} at ({chunk_x}, {chunk_z})",
                fixture_piece.bounding_box
            );
        }
    }
}

/// Every placed feature referenced by the sulfur caves biome must resolve in
/// the feature registry, so biome decoration can actually place the sulfur
/// spike/spring/pool features.
#[test]
fn sulfur_caves_feature_lists_resolve_in_registry() {
    use steel_registry::REGISTRY;
    use steel_registry::RegistryExt;
    use steel_registry::vanilla_biomes;

    crate::bootstrap::init_globals_once();

    let sulfur_caves = &*vanilla_biomes::SULFUR_CAVES;
    let referenced: Vec<_> = sulfur_caves
        .features
        .iter()
        .flatten()
        .collect();
    assert!(
        referenced
            .iter()
            .any(|id| id.path.contains("sulfur")),
        "sulfur caves biome must reference sulfur features"
    );
    for id in referenced {
        assert!(
            REGISTRY.placed_features.by_key(id).is_some(),
            "sulfur caves references unknown placed feature {id}"
        );
    }
}

/// The overworld multi-noise climate layout must actually produce the sulfur
/// caves biome somewhere near spawn scale, matching vanilla's cave-biome
/// placement. Release scan; marked ignored for normal test runs.
#[test]
#[ignore = "release-only scan over a large chunk area"]
fn sulfur_caves_biome_is_sampleable_in_overworld() {
    use steel_registry::vanilla_biomes;
    use steel_worldgen::biomes::BiomeSourceKind;

    crate::bootstrap::init_globals_once();

    let source = BiomeSourceKind::overworld(13579);
    let mut sampler = source.chunk_sampler();

    // Scan chunk centers across the fixture scan range at cave depth.
    for chunk_z in -100..=100 {
        for chunk_x in -100..=100 {
            let block_x = chunk_x * 16 + 8;
            let block_z = chunk_z * 16 + 8;
            let biome = sampler.sample(block_x >> 2, -40 >> 2, block_z >> 2);
            if biome.key == vanilla_biomes::SULFUR_CAVES.key {
                return;
            }
        }
    }
    panic!("sulfur caves biome never sampled in 201x201 chunk scan");
}
