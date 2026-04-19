//! Ocean monument piece generation.
//!
//! Vanilla `OceanMonumentStructure` produces a single `MonumentBuilding` piece
//! at `(chunkMinX - 29, 39, chunkMinZ - 29)` with size 58×23×58. Rotation is
//! chosen but doesn't affect the bounding box because the footprint is square.
//!
//! The monument has a special biome check: every biome in a 29-block radius
//! (in all three axes) around `(chunkMinX + 9, seaLevel, chunkMinZ + 9)` must
//! be in `#minecraft:required_ocean_monument_surrounding` — deep oceans,
//! regular oceans, and rivers.

use steel_utils::density::DimensionNoises;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Identifier};

use crate::world::structure::placement::StructureSelectionEntry;
use crate::world::structure::{GenerationContext, GenerationStub, Structure, StructurePiece};

/// Biomes that are allowed within the 29-block surrounding check.
/// Corresponds to `#minecraft:required_ocean_monument_surrounding`.
const SURROUNDING_BIOMES: &[&str] = &[
    "deep_frozen_ocean",
    "deep_cold_ocean",
    "deep_ocean",
    "deep_lukewarm_ocean",
    "frozen_ocean",
    "cold_ocean",
    "ocean",
    "lukewarm_ocean",
    "warm_ocean",
    "river",
    "frozen_river",
];

/// `Structure` impl — registered under `"minecraft:ocean_monument"`.
pub struct OceanMonumentStructure;

impl<N: DimensionNoises> Structure<N> for OceanMonumentStructure {
    fn find_generation_point(
        &self,
        ctx: &mut GenerationContext<'_, '_, N>,
        entry: &StructureSelectionEntry,
        _rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        // 29-block radius surrounding check (vanilla's "required_ocean_monument_surrounding").
        let check_x = ctx.chunk_min_x + 9;
        let check_z = ctx.chunk_min_z + 9;
        let check_y = ctx.sea_level;
        let radius = 29;

        // Quart-coord bounds of the 3D sweep: [min, max] for each axis.
        let x_range = ((check_x - radius) >> 2)..=((check_x + radius) >> 2);
        let z_range = ((check_z - radius) >> 2)..=((check_z + radius) >> 2);
        let y_range = ((check_y - radius) >> 2)..=((check_y + radius) >> 2);

        for qz in z_range {
            for qx in x_range.clone() {
                for qy in y_range.clone() {
                    let biome = ctx.biome_sampler.sample(qx, qy, qz);
                    let is_surrounding = SURROUNDING_BIOMES
                        .iter()
                        .any(|&b| biome.key == Identifier::vanilla_static(b));
                    if !is_surrounding {
                        return None;
                    }
                }
            }
        }

        // Center-biome check against the entry's allowed_biomes.
        let biome = ctx.biome_at(ctx.center_block_x, ctx.surface_y, ctx.center_block_z);
        if !entry.allowed_biomes.contains(&biome.key) {
            return None;
        }

        // Single MonumentBuilding piece. Rotation doesn't matter since
        // size is 58×23×58 (square footprint).
        let west = ctx.chunk_min_x - 29;
        let north = ctx.chunk_min_z - 29;
        let bb = BoundingBox::new(west, 39, north, west + 57, 61, north + 57);
        Some(GenerationStub {
            position: (ctx.center_block_x, ctx.surface_y, ctx.center_block_z),
            pieces: vec![StructurePiece {
                piece_type: Identifier::new_static("minecraft", "omb"),
                bounding_box: bb,
                gen_depth: 0,
                orientation: None,
                nbt_data: Vec::new(),
                ground_level_delta: 0,
                junctions: Vec::new(),
            }],
        })
    }
}
