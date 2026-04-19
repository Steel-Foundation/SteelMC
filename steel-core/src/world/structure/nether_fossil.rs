//! Nether fossil piece generation.
//!
//! Matches vanilla's `NetherFossilStructure.findGenerationPoint`: sample a
//! random (blockX, blockZ) in the chunk and a uniform Y, then walk the base
//! noise column down until we find an air block sitting on a solid block.
//! If the walk reaches sea level, no structure is placed.

use steel_utils::Identifier;
use steel_utils::Rotation;
use steel_utils::density::{DimensionNoises, NoiseSettings};
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;

use crate::world::structure::placement::StructureSelectionEntry;
use crate::world::structure::{
    ColumnBlock, GenerationContext, GenerationStub, Structure, StructurePiece,
};

/// Number of fossil template variants in `minecraft:nether_fossils/fossil_N`.
pub const FOSSIL_COUNT: i32 = 14;
const SEA_LEVEL: i32 = 32;

/// Result of a successful `find_generation_point` call.
pub struct FossilResult {
    /// Template name relative to `minecraft:` (e.g. "`nether_fossils/fossil_3`").
    pub template_name: String,
    /// World-space position where the piece sits (solid-block Y).
    pub position: (i32, i32, i32),
    /// Piece rotation.
    pub rotation: Rotation,
    /// Position used for the biome check.
    pub biome_check_pos: (i32, i32, i32),
}

/// Runs the full `findGenerationPoint` RNG sequence. Returns `None` when the
/// walk fails (no air-over-solid transition above sea level).
///
/// `min_gen_y` and `gen_depth` come from the dimension — for nether those are
/// `0` and `128`. The height range is `[32, gen_depth - 1 + min_gen_y - 2]`.
pub fn find_generation_point<F>(
    rng: &mut LegacyRandom,
    chunk_x: i32,
    chunk_z: i32,
    min_gen_y: i32,
    gen_depth: i32,
    mut get_column_state: F,
) -> Option<FossilResult>
where
    F: FnMut(i32, i32, i32) -> ColumnBlock,
{
    let chunk_min_x = chunk_x << 4;
    let chunk_min_z = chunk_z << 4;
    let block_x = chunk_min_x + rng.next_i32_bounded(16);
    let block_z = chunk_min_z + rng.next_i32_bounded(16);

    // UniformHeight.sample(absolute(32), below_top(2))
    // min = 32; max = gen_depth - 1 + min_gen_y - 2.
    // Mth.randomBetweenInclusive(rng, min, max) = min + nextInt(max - min + 1).
    let min = 32;
    let max = gen_depth - 1 + min_gen_y - 2;
    let mut y = if min > max {
        min
    } else {
        min + rng.next_i32_bounded(max - min + 1)
    };

    // Walk down. Vanilla reads `current = column.getBlock(y)` then
    // `below = column.getBlock(--y)`, breaking when current is air and below
    // has a sturdy-up face. In the base-noise column, soul_sand never appears,
    // so the break condition simplifies to "air over solid".
    let mut found = false;
    while y > SEA_LEVEL {
        let current = get_column_state(block_x, y, block_z);
        y -= 1;
        let below = get_column_state(block_x, y, block_z);
        if current == ColumnBlock::Air && below == ColumnBlock::Solid {
            found = true;
            break;
        }
    }

    // Vanilla also rejects the start if the break landed at sea level or below —
    // the `below` position (solid block) is `y`, which after `--y` can be `SEA_LEVEL`.
    if !found || y <= SEA_LEVEL {
        return None;
    }

    // `y` is the solid block's Y, matching vanilla's returned position.
    let rotation = Rotation::get_random(rng);
    let fossil_idx = rng.next_i32_bounded(FOSSIL_COUNT) + 1;
    let template_name = format!("nether_fossils/fossil_{fossil_idx}");

    Some(FossilResult {
        template_name,
        position: (block_x, y, block_z),
        rotation,
        biome_check_pos: (block_x, y, block_z),
    })
}

/// `Structure` impl — the entry point used by the `VanillaGenerator` dispatch.
pub struct NetherFossilStructure;

impl<N: DimensionNoises> Structure<N> for NetherFossilStructure {
    fn find_generation_point(
        &self,
        ctx: &mut GenerationContext<'_, '_, N>,
        entry: &StructureSelectionEntry,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let min_gen_y = <N::Settings as NoiseSettings>::MIN_Y;
        let gen_depth = <N::Settings as NoiseSettings>::HEIGHT;
        let (chunk_x, chunk_z) = (ctx.chunk_x, ctx.chunk_z);

        let result =
            find_generation_point(rng, chunk_x, chunk_z, min_gen_y, gen_depth, |x, y, z| {
                ctx.column_state(x, y, z)
            })?;

        let (bx, by, bz) = result.biome_check_pos;
        let biome = ctx.biome_at(bx, by, bz);
        if !entry.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let tmpl_id = Identifier::new("minecraft", result.template_name.clone());
        let tmpl = ctx.templates.get(&tmpl_id)?;
        let bb = result.rotation.get_bounding_box(
            result.position.0,
            result.position.1,
            result.position.2,
            tmpl.size[0],
            tmpl.size[1],
            tmpl.size[2],
        );
        Some(GenerationStub {
            position: result.position,
            pieces: vec![StructurePiece {
                piece_type: Identifier::new_static("minecraft", "nefos"),
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
