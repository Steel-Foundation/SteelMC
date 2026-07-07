use glam::IVec3;
use rustc_hash::FxHashMap;
use steel_registry::biome::BiomeRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::template_pool::{TemplateData, TemplatePoolData};
use steel_registry::{REGISTRY, RegistryExt, vanilla_biomes};
use steel_utils::random::RandomSource;
use steel_utils::{BlockStateId, ChunkPos, Identifier};

use crate::behavior::BlockStateBehaviorExt as _;
use crate::chunk::chunk_access::ChunkAccess;
use crate::worldgen::generator::{ChunkGenerator, xoroshiro_worldgen_region_random};
use crate::worldgen::region::WorldGenRegion;
use crate::worldgen::structure::{StructureGenerator, create_structures};
use steel_worldgen::noise::Beardifier;
use steel_worldgen::structure::{ColumnBlock, StructureGenerationContext};

/// A chunk generator that generates a flat world.
///
/// Uses a fixed biome (plains) for all positions, matching vanilla's
/// `FlatLevelSource` with `FixedBiomeSource`.
pub struct FlatChunkGenerator {
    /// Block layers from world bottom upwards.
    pub layers: Vec<BlockStateId>,
    /// The biome ID for plains (cached at construction).
    biome_id: u16,
    /// World seed for structure placement.
    seed: i64,
    /// Sea level for this flat generator's dimension type.
    sea_level: i32,
    /// Optional structure engine from flat structure overrides.
    structure_generator: Option<StructureGenerator>,
}

impl FlatChunkGenerator {
    /// Creates a new `FlatChunkGenerator`.
    #[must_use]
    pub fn new(bedrock: BlockStateId, dirt: BlockStateId, grass: BlockStateId) -> Self {
        Self::new_layers(vec![bedrock, dirt, dirt, grass])
    }

    /// Creates a new flat generator with explicit block layers from bottom upwards.
    #[must_use]
    pub fn new_layers(layers: Vec<BlockStateId>) -> Self {
        Self::new_layers_with_structures(layers, 0, 63, None)
    }

    /// Creates a flat generator with optional structure generation.
    #[must_use]
    pub(crate) fn new_layers_with_structures(
        layers: Vec<BlockStateId>,
        seed: i64,
        sea_level: i32,
        structure_generator: Option<StructureGenerator>,
    ) -> Self {
        let biome_id = REGISTRY
            .biomes
            .id_from_key(&Identifier::vanilla("plains".to_string()))
            .unwrap_or(0) as u16;

        Self {
            layers,
            biome_id,
            seed,
            sea_level,
            structure_generator,
        }
    }
}

struct FlatGenerationContext<'a> {
    seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    chunk_min_x: i32,
    chunk_min_z: i32,
    center_block_x: i32,
    center_block_z: i32,
    sea_level: i32,
    min_y: i32,
    height: i32,
    layers: &'a [BlockStateId],
    biome: BiomeRef,
    template_pools: &'a FxHashMap<Identifier, TemplatePoolData>,
    templates: &'a FxHashMap<Identifier, TemplateData>,
    surface_y_cache: Option<i32>,
}

impl FlatGenerationContext<'_> {
    fn state_at_y(&self, y: i32) -> Option<BlockStateId> {
        let relative_y = y.checked_sub(self.min_y)? as usize;
        self.layers.get(relative_y).copied()
    }

    fn is_opaque_at_y(&self, y: i32, ocean_floor: bool) -> bool {
        let Some(state) = self.state_at_y(y) else {
            return false;
        };
        if ocean_floor {
            state.is_solid()
        } else {
            state.is_solid() || state.has_fluid()
        }
    }

    fn base_height_flat(&self, ocean_floor: bool) -> i32 {
        for y in (self.min_y..self.min_y + self.height).rev() {
            if self.is_opaque_at_y(y, ocean_floor) {
                return y + 1;
            }
        }
        self.min_y
    }
}

impl StructureGenerationContext for FlatGenerationContext<'_> {
    fn seed(&self) -> i64 {
        self.seed
    }

    fn chunk_x(&self) -> i32 {
        self.chunk_x
    }

    fn chunk_z(&self) -> i32 {
        self.chunk_z
    }

    fn chunk_min_x(&self) -> i32 {
        self.chunk_min_x
    }

    fn chunk_min_z(&self) -> i32 {
        self.chunk_min_z
    }

    fn center_block_x(&self) -> i32 {
        self.center_block_x
    }

    fn center_block_z(&self) -> i32 {
        self.center_block_z
    }

    fn sea_level(&self) -> i32 {
        self.sea_level
    }

    fn min_y(&self) -> i32 {
        self.min_y
    }

    fn height(&self) -> i32 {
        self.height
    }

    fn template_pools(&self) -> &FxHashMap<Identifier, TemplatePoolData> {
        self.template_pools
    }

    fn templates(&self) -> &FxHashMap<Identifier, TemplateData> {
        self.templates
    }

    fn base_height(&mut self, _x: i32, _z: i32, ocean_floor: bool) -> i32 {
        self.base_height_flat(ocean_floor)
    }

    fn base_height_full(&mut self, _x: i32, _z: i32, ocean_floor: bool) -> i32 {
        self.base_height_flat(ocean_floor)
    }

    fn biome_at(&mut self, _block_x: i32, _block_y: i32, _block_z: i32) -> BiomeRef {
        self.biome
    }

    fn column_state(&mut self, _x: i32, y: i32, _z: i32) -> ColumnBlock {
        let Some(state) = self.state_at_y(y) else {
            return ColumnBlock::Air;
        };
        if state.is_solid() {
            ColumnBlock::Solid
        } else if state.has_fluid() {
            ColumnBlock::Fluid
        } else {
            ColumnBlock::Air
        }
    }

    fn surface_y(&mut self) -> i32 {
        if let Some(y) = self.surface_y_cache {
            return y;
        }
        let y = self.base_height_flat(false) - 1;
        self.surface_y_cache = Some(y);
        y
    }

    fn terrain_surface_height(&self, _x: i32, _z: i32, ocean_floor: bool) -> i32 {
        self.base_height_flat(ocean_floor)
    }

    fn terrain_is_opaque(&self, _x: i32, y: i32, _z: i32, ocean_floor: bool) -> bool {
        self.is_opaque_at_y(y, ocean_floor)
    }
}

impl ChunkGenerator for FlatChunkGenerator {
    fn min_y(&self) -> i32 {
        0
    }

    fn gen_depth(&self) -> i32 {
        384
    }

    fn spawn_height(&self, min_y: i32, height: i32) -> i32 {
        min_y + height.min(self.layers.len() as i32)
    }

    fn structure_generator(&self) -> Option<&StructureGenerator> {
        self.structure_generator.as_ref()
    }

    fn create_structures(&self, chunk: &ChunkAccess) {
        let Some(structure_generator) = &self.structure_generator else {
            return;
        };

        let pos = chunk.pos();
        let chunk_x = pos.0.x;
        let chunk_z = pos.0.y;
        let chunk_min_x = chunk_x * 16;
        let chunk_min_z = chunk_z * 16;
        let mut ctx = FlatGenerationContext {
            seed: self.seed,
            chunk_x,
            chunk_z,
            chunk_min_x,
            chunk_min_z,
            center_block_x: chunk_min_x + 8,
            center_block_z: chunk_min_z + 8,
            sea_level: self.sea_level,
            min_y: chunk.min_y(),
            height: (chunk.sections().sections.len() * 16) as i32,
            layers: &self.layers,
            biome: &vanilla_biomes::PLAINS,
            template_pools: structure_generator.template_pools(),
            templates: structure_generator.templates(),
            surface_y_cache: None,
        };
        create_structures(structure_generator, chunk, &mut ctx);
    }

    fn create_biomes(&self, chunk: &ChunkAccess) {
        let section_count = chunk.sections().sections.len();

        for section_index in 0..section_count {
            let section = &chunk.sections().sections[section_index];
            let mut section_guard = section.write();

            for local_quart_x in 0..4usize {
                for local_quart_y in 0..4usize {
                    for local_quart_z in 0..4usize {
                        section_guard.biomes.set(
                            local_quart_x,
                            local_quart_y,
                            local_quart_z,
                            self.biome_id,
                        );
                    }
                }
            }
            drop(section_guard);
        }

        chunk.mark_dirty();
    }

    fn fill_from_noise(&self, chunk: &ChunkAccess, _beardifier: Option<&Beardifier>) {
        let max_relative_y = chunk.sections().sections.len() * 16;

        for x in 0..16 {
            for z in 0..16 {
                for (relative_y, block) in self.layers.iter().enumerate().take(max_relative_y) {
                    chunk.set_relative_block_for_generation(x, relative_y, z, *block);
                }
            }
        }
    }

    fn build_surface(&self, _chunk: &ChunkAccess, _neighbor_biomes: &dyn Fn(IVec3) -> u16) {}

    fn apply_carvers(&self, _chunk: &ChunkAccess) {}

    fn create_worldgen_region_random(&self, world_seed: i64, center: ChunkPos) -> RandomSource {
        xoroshiro_worldgen_region_random(world_seed, center)
    }

    fn apply_biome_decorations(&self, _region: &mut WorldGenRegion<'_>) {}
}
