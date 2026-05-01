//! Shipwreck: picks a random template from the beached (11) or underwater (20) pool,
//! places at `(chunkMinX, 90, chunkMinZ)` with random rotation and pivot `(4, 15)`.

use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{Direction, Identifier, Rotation};
use steel_worldgen::density::DimensionNoises;

use crate::world::structure::placement::StructureSelectionEntry;
use crate::world::structure::{GenerationContext, GenerationStub, Structure, StructurePiece};

static BEACHED: &[&str] = &[
    "shipwreck/with_mast",
    "shipwreck/sideways_full",
    "shipwreck/sideways_fronthalf",
    "shipwreck/sideways_backhalf",
    "shipwreck/rightsideup_full",
    "shipwreck/rightsideup_fronthalf",
    "shipwreck/rightsideup_backhalf",
    "shipwreck/with_mast_degraded",
    "shipwreck/rightsideup_full_degraded",
    "shipwreck/rightsideup_fronthalf_degraded",
    "shipwreck/rightsideup_backhalf_degraded",
];

static OCEAN: &[&str] = &[
    "shipwreck/with_mast",
    "shipwreck/upsidedown_full",
    "shipwreck/upsidedown_fronthalf",
    "shipwreck/upsidedown_backhalf",
    "shipwreck/sideways_full",
    "shipwreck/sideways_fronthalf",
    "shipwreck/sideways_backhalf",
    "shipwreck/rightsideup_full",
    "shipwreck/rightsideup_fronthalf",
    "shipwreck/rightsideup_backhalf",
    "shipwreck/with_mast_degraded",
    "shipwreck/upsidedown_full_degraded",
    "shipwreck/upsidedown_fronthalf_degraded",
    "shipwreck/upsidedown_backhalf_degraded",
    "shipwreck/sideways_full_degraded",
    "shipwreck/sideways_fronthalf_degraded",
    "shipwreck/sideways_backhalf_degraded",
    "shipwreck/rightsideup_full_degraded",
    "shipwreck/rightsideup_fronthalf_degraded",
    "shipwreck/rightsideup_backhalf_degraded",
];

/// Registered under `"minecraft:shipwreck"`. Beached vs underwater is distinguished by
/// `entry.structure.path`.
pub struct ShipwreckStructure;

impl<N: DimensionNoises> Structure<N> for ShipwreckStructure {
    fn find_generation_point(
        &self,
        ctx: &mut GenerationContext<'_, '_, N>,
        entry: &StructureSelectionEntry,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let surface_y = ctx.surface_y();
        let biome = ctx.biome_at(ctx.center_block_x, surface_y, ctx.center_block_z);
        if !entry.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let templates_arr = if &*entry.structure.path == "shipwreck_beached" {
            BEACHED
        } else {
            OCEAN
        };

        let rotation = Rotation::get_random(rng);
        let idx = rng.next_i32_bounded(templates_arr.len() as i32) as usize;
        let template_id = Identifier::new("minecraft", templates_arr[idx].to_string());
        let tmpl = ctx.templates.get(&template_id)?;

        Some(GenerationStub {
            position: (ctx.center_block_x, surface_y, ctx.center_block_z),
            pieces: vec![StructurePiece {
                piece_type: Identifier::new_static("minecraft", "shipwreck"),
                bounding_box: rotation.get_bounding_box_with_pivot(
                    (ctx.chunk_min_x, 90, ctx.chunk_min_z),
                    (tmpl.size[0], tmpl.size[1], tmpl.size[2]),
                    4,
                    15,
                ),
                gen_depth: 0,
                orientation: Some(Direction::North),
                nbt_data: Vec::new(),
                ground_level_delta: 0,
                junctions: Vec::new(),
                projection: None,
            }],
        })
    }
}
