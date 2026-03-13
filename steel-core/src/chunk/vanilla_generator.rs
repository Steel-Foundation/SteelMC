use std::marker::PhantomData;

use steel_registry::REGISTRY;
use steel_registry::noise_parameters::get_noise_parameters;
use steel_utils::BlockStateId;
use steel_utils::density::{ColumnCache, DimensionNoises, NoiseSettings};
use steel_utils::random::{Random, RandomSplitter, xoroshiro::Xoroshiro};

use crate::chunk::aquifer::{Aquifer, AquiferResult};
use crate::chunk::beardifier::Beardifier;
use crate::chunk::chunk_access::ChunkAccess;
use crate::chunk::chunk_generator::ChunkGenerator;
use crate::chunk::noise_chunk::NoiseChunk;
use crate::chunk::ore_veinifier::OreVeinifier;
use crate::worldgen::BiomeSourceKind;

/// A chunk generator for vanilla (normal) world generation.
///
/// Matches vanilla's `NoiseBasedChunkGenerator`. The biome source is pluggable
/// per-dimension — overworld, nether, and end each provide a different
/// [`BiomeSourceKind`] variant.
///
/// Generic over `N: DimensionNoises` to support different dimensions with
/// their own transpiled density functions and noise settings.
pub struct VanillaGenerator<N: DimensionNoises> {
    /// Biome source for this dimension. Determines biomes at each quart position.
    biome_source: BiomeSourceKind,
    /// Noise generators for this dimension's density functions.
    /// Boxed because noise structs can be large.
    noises: Box<N>,
    /// Seed positional splitter for per-chunk construction of aquifers.
    splitter: RandomSplitter,
    /// Ore vein generator for replacing stone with ore blocks.
    ore_veinifier: Option<OreVeinifier>,
    /// Block state ID for the default block, cached at construction time.
    default_block_id: BlockStateId,
    _phantom: PhantomData<N>,
}

impl<N: DimensionNoises> VanillaGenerator<N> {
    /// Creates a new `VanillaGenerator` with the given biome source and seed.
    #[must_use]
    pub fn new(biome_source: BiomeSourceKind, seed: u64) -> Self {
        let mut rng = Xoroshiro::from_seed(seed);
        let splitter = rng.next_positional();
        let noise_params = get_noise_parameters();
        let noises = N::create(seed, &splitter, &noise_params);

        let ore_veinifier = if N::Settings::ORE_VEINS_ENABLED {
            Some(OreVeinifier::new(&splitter))
        } else {
            None
        };

        Self {
            biome_source,
            noises: Box::new(noises),
            splitter,
            ore_veinifier,
            default_block_id: N::Settings::default_block_id(),
            _phantom: PhantomData,
        }
    }
}

impl<N: DimensionNoises> ChunkGenerator for VanillaGenerator<N> {
    fn create_structures(&self, _chunk: &ChunkAccess) {}

    fn create_biomes(&self, chunk: &ChunkAccess) {
        let pos = chunk.pos();
        let min_y = chunk.min_y();
        let section_count = chunk.sections().sections.len();

        let chunk_x = pos.0.x;
        let chunk_z = pos.0.y;

        let mut sampler = self.biome_source.chunk_sampler();

        // Column-major iteration: sample all Y values for each (X, Z) column
        // before moving to the next column. This keeps the column cache effective —
        // column-level density functions (continents, erosion, ridges, etc.) are
        // computed once per column instead of once per sample.
        for local_quart_x in 0..4i32 {
            for local_quart_z in 0..4i32 {
                let quart_x = chunk_x * 4 + local_quart_x;
                let quart_z = chunk_z * 4 + local_quart_z;

                for section_index in 0..section_count {
                    let section_y = (min_y / 16) + section_index as i32;
                    let section = &chunk.sections().sections[section_index];
                    let mut section_guard = section.write();

                    for local_quart_y in 0..4i32 {
                        let quart_y = section_y * 4 + local_quart_y;

                        let biome = sampler.sample(quart_x, quart_y, quart_z);
                        let biome_id = *REGISTRY.biomes.get_id(biome) as u16;

                        section_guard.biomes.set(
                            local_quart_x as usize,
                            local_quart_y as usize,
                            local_quart_z as usize,
                            biome_id,
                        );
                    }
                }
            }
        }

        chunk.mark_dirty();
    }

    fn fill_from_noise(&self, chunk: &ChunkAccess) {
        let pos = chunk.pos();
        let chunk_min_x = pos.0.x * 16;
        let chunk_min_z = pos.0.y * 16;

        let min_y = N::Settings::MIN_Y;
        let height = N::Settings::HEIGHT;

        let mut noise_chunk = NoiseChunk::<N>::new(chunk_min_x, chunk_min_z);
        let noises = &*self.noises;

        let mut column_cache = N::ColumnCache::default();
        column_cache.init_grid(chunk_min_x, chunk_min_z, noises);

        let default_block_id = self.default_block_id;
        let ore_veinifier = &self.ore_veinifier;
        let mut aquifer = Aquifer::<N>::new(
            chunk_min_x,
            chunk_min_z,
            min_y,
            height,
            &self.splitter,
            noises,
            // Aquifer samples at arbitrary (x,z) outside the chunk, so it needs its own cache
            column_cache.clone(),
        );

        let structure_starts = chunk.structure_starts();
        let beardifier = Beardifier::for_structures_in_chunk(&structure_starts, pos.0.x, pos.0.y);
        let beard_opt = if beardifier.is_empty() {
            None
        } else {
            Some(&beardifier)
        };

        noise_chunk.fill(
            noises,
            &mut column_cache,
            beard_opt,
            |local_x, world_y, local_z, density, interpolated, cache| {
                let relative_y = (world_y - min_y) as usize;
                let world_x = chunk_min_x + local_x as i32;
                let world_z = chunk_min_z + local_z as i32;

                match aquifer.compute_substance(noises, world_x, world_y, world_z, density) {
                    AquiferResult::Solid => {
                        let block = ore_veinifier
                            .as_ref()
                            .and_then(|ov| {
                                ov.compute_interpolated(
                                    noises,
                                    cache,
                                    interpolated,
                                    world_x,
                                    world_y,
                                    world_z,
                                )
                            })
                            .unwrap_or(default_block_id);
                        chunk.set_relative_block(local_x, relative_y, local_z, block);
                    }
                    AquiferResult::Fluid(id) => {
                        chunk.set_relative_block(local_x, relative_y, local_z, id);
                    }
                    AquiferResult::Air => {}
                }
            },
        );
    }

    fn build_surface(&self, _chunk: &ChunkAccess) {}

    fn apply_carvers(&self, _chunk: &ChunkAccess) {}

    fn apply_biome_decorations(&self, _chunk: &ChunkAccess) {}
}
