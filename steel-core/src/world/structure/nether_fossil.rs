//! Nether fossil: sample a random (x, z) in the chunk and a uniform Y, then walk
//! the base-noise column down until we find air over solid. Fails if the walk
//! reaches sea level.

use steel_registry::structure::{
    HeightProviderData, StructureConfigData, StructureData, VerticalAnchorData,
};
use steel_utils::Direction;
use steel_utils::Identifier;
use steel_utils::Rotation;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;

use crate::world::structure::{
    ColumnBlock, GenerationStub, Structure, StructureGenerationContext, StructurePiece,
};

/// Fossil templates count (`minecraft:nether_fossils/fossil_N`).
pub const FOSSIL_COUNT: i32 = 14;
const SEA_LEVEL: i32 = 32;

/// Result of [`find_generation_point`].
pub struct FossilResult {
    /// Template path relative to `minecraft:` (e.g. `"nether_fossils/fossil_3"`).
    pub template_name: String,
    /// World-space solid-block position.
    pub position: (i32, i32, i32),
    /// Piece rotation.
    pub rotation: Rotation,
    /// Position used for the biome check.
    pub biome_check_pos: (i32, i32, i32),
}

const fn resolve_vertical_anchor(
    anchor: &VerticalAnchorData,
    min_gen_y: i32,
    gen_depth: i32,
) -> i32 {
    match anchor {
        VerticalAnchorData::Absolute(y) => *y,
        VerticalAnchorData::AboveBottom(offset) => min_gen_y + *offset,
        VerticalAnchorData::BelowTop(offset) => min_gen_y + gen_depth - 1 - *offset,
    }
}

fn sample_height(
    height: &HeightProviderData,
    rng: &mut LegacyRandom,
    min_gen_y: i32,
    gen_depth: i32,
) -> i32 {
    match height {
        HeightProviderData::Constant(anchor) => {
            resolve_vertical_anchor(anchor, min_gen_y, gen_depth)
        }
        HeightProviderData::Uniform {
            min_inclusive,
            max_inclusive,
        } => {
            let min = resolve_vertical_anchor(min_inclusive, min_gen_y, gen_depth);
            let max = resolve_vertical_anchor(max_inclusive, min_gen_y, gen_depth);
            if min > max {
                min
            } else {
                min + rng.next_i32_bounded(max - min + 1)
            }
        }
    }
}

/// Vanilla's RNG sequence. Returns `None` if no air-over-solid transition above sea level.
pub fn find_generation_point<F>(
    rng: &mut LegacyRandom,
    chunk_x: i32,
    chunk_z: i32,
    height: &HeightProviderData,
    min_gen_y: i32,
    gen_depth: i32,
    mut get_column_state: F,
) -> Option<FossilResult>
where
    F: FnMut(i32, i32, i32) -> ColumnBlock,
{
    let block_x = (chunk_x << 4) + rng.next_i32_bounded(16);
    let block_z = (chunk_z << 4) + rng.next_i32_bounded(16);

    let mut y = sample_height(height, rng, min_gen_y, gen_depth);

    // Base-noise column has no soul_sand, so the vanilla sturdy-face check = Solid.
    let mut found = false;
    while y > SEA_LEVEL {
        let current = get_column_state(block_x, y, block_z);
        y -= 1;
        if current == ColumnBlock::Air
            && get_column_state(block_x, y, block_z) == ColumnBlock::Solid
        {
            found = true;
            break;
        }
    }

    if !found || y <= SEA_LEVEL {
        return None;
    }

    let rotation = Rotation::get_random(rng);
    let fossil_idx = rng.next_i32_bounded(FOSSIL_COUNT) + 1;
    Some(FossilResult {
        template_name: format!("nether_fossils/fossil_{fossil_idx}"),
        position: (block_x, y, block_z),
        rotation,
        biome_check_pos: (block_x, y, block_z),
    })
}

/// Entry point used by `VanillaGenerator`.
pub struct NetherFossilStructure;

impl Structure for NetherFossilStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let min_gen_y = ctx.min_y();
        let gen_depth = ctx.height();
        let (chunk_x, chunk_z) = (ctx.chunk_x(), ctx.chunk_z());
        let StructureConfigData::NetherFossil { height } = &structure.config else {
            return None;
        };

        let result = find_generation_point(
            rng,
            chunk_x,
            chunk_z,
            height,
            min_gen_y,
            gen_depth,
            |x, y, z| ctx.column_state(x, y, z),
        )?;

        let (bx, by, bz) = result.biome_check_pos;
        let biome = ctx.biome_at(bx, by, bz);
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let tmpl = ctx
            .templates()
            .get(&Identifier::new("minecraft", result.template_name.clone()))?;
        Some(GenerationStub {
            position: result.position,
            pieces: vec![StructurePiece::non_jigsaw(
                Identifier::new_static("minecraft", "nefos"),
                result.rotation.get_bounding_box(
                    result.position.0,
                    result.position.1,
                    result.position.2,
                    tmpl.size[0],
                    tmpl.size[1],
                    tmpl.size[2],
                ),
                0,
                Some(Direction::North),
            )],
        })
    }
}
