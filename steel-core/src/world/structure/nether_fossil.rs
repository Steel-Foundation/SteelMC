//! Nether fossil: sample a random (x, z) in the chunk and a uniform Y, then walk
//! the base-noise column down until we find air over solid. Fails if the walk
//! reaches sea level.

use steel_utils::Identifier;
use steel_utils::Rotation;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_worldgen::density::{DimensionNoises, NoiseSettings};

use crate::world::structure::placement::StructureSelectionEntry;
use crate::world::structure::{
    ColumnBlock, GenerationContext, GenerationStub, Structure, StructurePiece,
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

/// Vanilla's RNG sequence. Returns `None` if no air-over-solid transition above sea level.
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
    let block_x = (chunk_x << 4) + rng.next_i32_bounded(16);
    let block_z = (chunk_z << 4) + rng.next_i32_bounded(16);

    // UniformHeight.sample(absolute(32), below_top(2)) = [32, gen_depth-1+min_gen_y-2].
    let min = 32;
    let max = gen_depth - 1 + min_gen_y - 2;
    let mut y = if min > max {
        min
    } else {
        min + rng.next_i32_bounded(max - min + 1)
    };

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

        let tmpl = ctx
            .templates
            .get(&Identifier::new("minecraft", result.template_name.clone()))?;
        Some(GenerationStub {
            position: result.position,
            pieces: vec![StructurePiece {
                piece_type: Identifier::new_static("minecraft", "nefos"),
                bounding_box: result.rotation.get_bounding_box(
                    result.position.0,
                    result.position.1,
                    result.position.2,
                    tmpl.size[0],
                    tmpl.size[1],
                    tmpl.size[2],
                ),
                gen_depth: 0,
                orientation: None,
                nbt_data: Vec::new(),
                ground_level_delta: 0,
                junctions: Vec::new(),
            }],
        })
    }
}
