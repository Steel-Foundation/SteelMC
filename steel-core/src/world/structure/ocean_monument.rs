//! Ocean monument: single 58×23×58 `MonumentBuilding` at `(chunkMinX-29, 39, chunkMinZ-29)`.
//! Square footprint so rotation doesn't affect the bounding box.
//!
//! Special biome check: every biome in a 29-block (3D) radius around
//! `(chunkMinX+9, seaLevel, chunkMinZ+9)` must be in `#required_ocean_monument_surrounding`.

use steel_registry::structure::StructureData;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Identifier};

use crate::world::structure::{
    GenerationStub, Structure, StructureGenerationContext, StructurePiece,
    random_horizontal_direction,
};

/// `#minecraft:required_ocean_monument_surrounding`.
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

/// Registered under `"minecraft:ocean_monument"`.
pub struct OceanMonumentStructure;

impl Structure for OceanMonumentStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let check_x = ctx.chunk_min_x() + 9;
        let check_z = ctx.chunk_min_z() + 9;
        let check_y = ctx.sea_level();
        let radius = 29;

        let x_range = ((check_x - radius) >> 2)..=((check_x + radius) >> 2);
        let z_range = ((check_z - radius) >> 2)..=((check_z + radius) >> 2);
        let y_range = ((check_y - radius) >> 2)..=((check_y + radius) >> 2);

        for qz in z_range {
            for qx in x_range.clone() {
                for qy in y_range.clone() {
                    let biome = ctx.biome_at(qx << 2, qy << 2, qz << 2);
                    if !SURROUNDING_BIOMES
                        .iter()
                        .any(|&b| biome.key == Identifier::vanilla_static(b))
                    {
                        return None;
                    }
                }
            }
        }

        let surface_y = ctx.surface_y();
        let biome = ctx.biome_at(ctx.center_block_x(), surface_y, ctx.center_block_z());
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let west = ctx.chunk_min_x() - 29;
        let north = ctx.chunk_min_z() - 29;
        let orientation = random_horizontal_direction(rng);
        Some(GenerationStub {
            position: (ctx.center_block_x(), surface_y, ctx.center_block_z()),
            pieces: vec![StructurePiece::non_jigsaw(
                Identifier::new_static("minecraft", "omb"),
                BoundingBox::new(west, 39, north, west + 57, 61, north + 57),
                0,
                Some(orientation),
            )],
        })
    }
}
