use std::{path::Path, sync::Arc};

use glam::IVec3;
use steel_core::{
    chunk::Chunk,
    worldgen::{
        ChunkGenerator, ChunkGeneratorType, GeneratorOutput, WorldGenRegion,
        generator::{
            GenerationChunk, TerrainPhase, registry::CustomWorldGeneratorFactory,
            xoroshiro_worldgen_region_random,
        },
    },
};
use steel_registry::{
    REGISTRY, RegistryEntry, RegistryExt, biome::BiomeRef, dimension_type::DimensionTypeRef,
};
use steel_utils::{
    BlockStateId, ChunkPos, Identifier,
    random::{PositionalRandom, Random, RandomSource, xoroshiro::Xoroshiro},
};
use steel_worldgen::noise::Beardifier;

use super::TerrainAccess;
use crate::{BlockChange, BlockPos, Terrain, TerrainCallback};
use steel_core::worldgen::WorldGeneratorRegistry;

pub(crate) fn register_generator(
    registry: &mut WorldGeneratorRegistry,
    key: &str,
    dimension: &str,
    biome: &str,
    sea_level: i32,
    callback: TerrainCallback,
) -> Result<(), String> {
    super::ensure_registry_access()?;
    let key: Identifier = key.parse().map_err(str::to_owned)?;
    let dimension: Identifier = dimension.parse().map_err(str::to_owned)?;
    let biome: Identifier = biome.parse().map_err(str::to_owned)?;
    let dimension_type = REGISTRY
        .dimension_types
        .by_key(&dimension)
        .ok_or_else(|| format!("unknown dimension type {dimension}"))?;
    let biome = REGISTRY
        .biomes
        .by_key(&biome)
        .ok_or_else(|| format!("unknown biome {biome}"))?;
    let biome_id = u16::try_from(biome.id())
        .map_err(|_| format!("biome {} exceeds palette ID range", biome.key))?;
    let factory = TerrainFactory {
        dimension_type,
        biome,
        biome_id,
        sea_level,
        callback,
    };
    registry.register_custom(key, Arc::new(factory))
}

struct TerrainFactory {
    dimension_type: DimensionTypeRef,
    biome: BiomeRef,
    biome_id: u16,
    sea_level: i32,
    callback: TerrainCallback,
}

impl CustomWorldGeneratorFactory for TerrainFactory {
    fn dimension_type(&self) -> DimensionTypeRef {
        self.dimension_type
    }

    fn validate(&self, config: &toml::Value) -> Result<toml::Value, String> {
        let Some(table) = config.as_table() else {
            return Err("terrain generator config must be a table".to_owned());
        };
        if !table.is_empty() {
            return Err("this terrain generator accepts no per-world config".to_owned());
        }
        Ok(config.clone())
    }

    fn create(
        &self,
        _path: Option<&Path>,
        config: &toml::Value,
        seed: i64,
        _pool: &rayon::ThreadPool,
    ) -> Result<GeneratorOutput, String> {
        Ok(GeneratorOutput {
            dimension_type: self.dimension_type,
            config: config.clone(),
            is_flat: false,
            sea_level: self.sea_level,
            generator: ChunkGeneratorType::Custom(Box::new(TerrainGenerator {
                dimension_type: self.dimension_type,
                biome: self.biome,
                biome_id: self.biome_id,
                seed,
                callback: self.callback,
            })),
        })
    }
}

/// A complete terrain-only generator: uniform biomes, no structures or decorators.
struct TerrainGenerator {
    dimension_type: DimensionTypeRef,
    biome: BiomeRef,
    biome_id: u16,
    seed: i64,
    callback: TerrainCallback,
}

impl ChunkGenerator for TerrainGenerator {
    fn min_y(&self) -> i32 {
        self.dimension_type.min_y
    }
    fn gen_depth(&self) -> i32 {
        self.dimension_type.height
    }
    fn noise_biome(&self, _x: i32, _y: i32, _z: i32) -> BiomeRef {
        self.biome
    }
    fn create_structures(&self, _chunk: &Chunk) {}

    fn create_biomes(&self, chunk: &Chunk) {
        for section in &chunk.sections().sections {
            let mut section = section.write();
            for x in 0..4 {
                for y in 0..4 {
                    for z in 0..4 {
                        section.biomes.set(x, y, z, self.biome_id);
                    }
                }
            }
        }
        chunk.mark_dirty();
    }

    fn fill_from_noise(
        &self,
        chunk: GenerationChunk<'_, TerrainPhase>,
        _beardifier: Option<&Beardifier>,
    ) {
        let access = TerrainAccess {
            chunk,
            seed: self.seed,
        };
        (self.callback)(Terrain::from_host(&access));
    }
    fn build_surface(
        &self,
        _chunk: GenerationChunk<'_, TerrainPhase>,
        _biomes: &dyn Fn(IVec3) -> u16,
    ) {
    }
    fn apply_carvers(&self, _chunk: GenerationChunk<'_, TerrainPhase>) {}
    fn apply_biome_decorations(&self, _region: &mut WorldGenRegion<'_>) {}

    fn create_worldgen_region_random(&self, world_seed: i64, center: ChunkPos) -> RandomSource {
        xoroshiro_worldgen_region_random(world_seed, center)
    }
}

pub(crate) fn random_at(seed: i64, salt: u64, pos: BlockPos) -> u64 {
    let mut named = Xoroshiro::from_seed(seed as u64)
        .next_positional()
        .with_seed(salt);
    named.next_positional().at(pos.x, pos.y, pos.z).next_i64() as u64
}

pub(crate) fn write_terrain(
    access: &TerrainAccess<'_>,
    changes: &[BlockChange],
) -> Result<(), String> {
    let min_x = access.chunk.pos().0.x * 16;
    let min_z = access.chunk.pos().0.y * 16;
    let min_y = access.chunk.min_y();
    let height = access.chunk.height();
    let mut blocks = Vec::with_capacity(changes.len());
    for change in changes {
        let Some(x) = change
            .pos
            .x
            .checked_sub(min_x)
            .filter(|x| (0..16).contains(x))
        else {
            return Err("terrain write is outside the current chunk's X range".to_owned());
        };
        let Some(z) = change
            .pos
            .z
            .checked_sub(min_z)
            .filter(|z| (0..16).contains(z))
        else {
            return Err("terrain write is outside the current chunk's Z range".to_owned());
        };
        let Some(y) = change
            .pos
            .y
            .checked_sub(min_y)
            .filter(|y| (0..height).contains(y))
        else {
            return Err("terrain write is outside the dimension's build height".to_owned());
        };
        let state = BlockStateId(change.state.id);
        if REGISTRY.blocks.by_state_id(state).is_none() {
            return Err("invalid terrain block state".to_owned());
        }
        blocks.push((x as usize, y as usize, z as usize, state));
    }
    access.chunk.write_block_batch(&blocks);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::random_at;
    use crate::BlockPos;

    #[test]
    fn positional_sampling_is_independent_of_call_order_and_other_world_seeds() {
        let positions = [
            BlockPos { x: 0, y: 0, z: 0 },
            BlockPos {
                x: -16,
                y: -64,
                z: 32,
            },
            BlockPos {
                x: 104,
                y: 90,
                z: -205,
            },
        ];
        let expected = positions.map(|pos| random_at(-12345, 17, pos));
        for index in (0..positions.len()).rev() {
            let pos = positions[index];
            assert_ne!(random_at(-12346, 17, pos), expected[index]);
            assert_ne!(random_at(-12345, 18, pos), expected[index]);
            assert_eq!(random_at(-12345, 17, pos), expected[index]);
        }
    }
}
