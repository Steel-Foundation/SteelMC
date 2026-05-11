//! Igloo: one top piece always, 50% chance of a basement (laboratory + `depth-1`
//! ladder segments, depth ∈ [4, 11]).

use steel_registry::structure::StructureData;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Direction, Identifier, Rotation};

use crate::world::structure::{
    GenerationStub, Structure, StructureGenerationContext, StructurePiece,
};

const TOP_SIZE: [i32; 3] = [7, 5, 8];
const MID_SIZE: [i32; 3] = [3, 3, 3];
const BOT_SIZE: [i32; 3] = [7, 6, 9];
const TOP_PIVOT: (i32, i32) = (3, 5);
const MID_PIVOT: (i32, i32) = (1, 1);
const BOT_PIVOT: (i32, i32) = (3, 7);
const TOP_OFF: (i32, i32, i32) = (0, 0, 0);
const MID_OFF: (i32, i32, i32) = (2, -3, 4);
const BOT_OFF: (i32, i32, i32) = (0, -3, -2);
const GEN_Y: i32 = 90;

const fn make_piece_bb(
    start_x: i32,
    start_z: i32,
    rotation: Rotation,
    off: (i32, i32, i32),
    depth: i32,
    size: [i32; 3],
    pivot: (i32, i32),
) -> BoundingBox {
    let pos_x = start_x + off.0;
    let pos_y = GEN_Y + off.1 - depth;
    let pos_z = start_z + off.2;
    rotation.get_bounding_box_with_pivot(
        (pos_x, pos_y, pos_z),
        (size[0], size[1], size[2]),
        pivot.0,
        pivot.1,
    )
}

const fn piece(bb: BoundingBox) -> StructurePiece {
    StructurePiece::non_jigsaw(
        Identifier::new_static("minecraft", "iglu"),
        bb,
        0,
        Some(Direction::North),
    )
}

/// Registered under `"minecraft:igloo"`.
pub struct IglooStructure;

impl Structure for IglooStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let surface_y = ctx.surface_y();
        let biome = ctx.biome_at(ctx.center_block_x(), surface_y, ctx.center_block_z());
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let rotation = Rotation::get_random(rng);
        let (start_x, start_z) = (ctx.chunk_min_x(), ctx.chunk_min_z());
        let mk = |off, depth, size, pivot| {
            piece(make_piece_bb(
                start_x, start_z, rotation, off, depth, size, pivot,
            ))
        };

        let mut pieces = Vec::new();
        if rng.next_f64() < 0.5_f64 {
            let depth = rng.next_i32_bounded(8) + 4;
            pieces.push(mk(BOT_OFF, depth * 3, BOT_SIZE, BOT_PIVOT));
            for i in 0..depth - 1 {
                pieces.push(mk(MID_OFF, i * 3, MID_SIZE, MID_PIVOT));
            }
        }
        pieces.push(mk(TOP_OFF, 0, TOP_SIZE, TOP_PIVOT));

        Some(GenerationStub {
            position: (ctx.center_block_x(), surface_y, ctx.center_block_z()),
            pieces,
        })
    }
}
