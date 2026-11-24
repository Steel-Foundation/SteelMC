use std::{io, sync::Arc};

use rustc_hash::FxHashMap;
use steel_math::trig;
use steel_registry::{
    REGISTRY, RegistryExt, TaggedRegistryExt, vanilla_biome_tags,
    vanilla_map_colors::{self as colors, brightness},
};
use steel_utils::{BlockPos, ChunkPos, locks::SyncMutex};
use steel_worldgen::biomes::obfuscate_biome_seed;

use super::{HALF_MAP_SIZE, MAP_SIZE, MapItemSavedData, chunk_io::MapChunk};
use crate::{
    chunk::{
        paletted_container::{BiomePalette, BlockPalette},
        status::ChunkStatus,
    },
    world::{LevelReader, World},
    worldgen::{ChunkGenerator, generator::vanilla::fuzzed_biome_quart_position},
};

const QUARTS_PER_SECTION_BITS: u32 = BiomePalette::SIZE.ilog2();
const BLOCKS_PER_QUART_BITS: u32 = (BlockPalette::SIZE / BiomePalette::SIZE).ilog2();
const OPEN_WATER_NEIGHBOR_THRESHOLD: i32 = 7;
const NORMAL_WATER_NEIGHBOR_THRESHOLD: i32 = 5;
const LOW_WATER_NEIGHBOR_THRESHOLD: i32 = 1;
const NORMAL_COAST_NEIGHBOR_THRESHOLD: i32 = 3;
const OCEAN_STRIPE_ROW_INTERVAL: i32 = 2;
const OCEAN_STRIPE_AMPLITUDE: f32 = 7.0;
const OCEAN_STRIPE_WIDTH: i32 = 8;
const OCEAN_STRIPE_PERIOD: i32 = 5;

impl MapItemSavedData {
    /// Uses available chunk biomes or the generator without loading missing
    /// chunks. Existing permitted generation can suspend without holding guards.
    pub async fn render_biome_preview(
        data: &Arc<SyncMutex<Self>>,
        world: &Arc<World>,
    ) -> io::Result<()> {
        let (center_x, center_z, scale) = {
            let data = data.lock();
            if data.dimension != world.key {
                return Ok(());
            }
            (
                data.center_x,
                data.center_z,
                1_i32.wrapping_shl(data.scale as u32),
            )
        };
        let start_x = center_x / scale - HALF_MAP_SIZE;
        let start_z = center_z / scale - HALF_MAP_SIZE;
        let seed = obfuscate_biome_seed(world.seed());
        let mut watery = Box::new([false; MAP_SIZE * MAP_SIZE]);
        let mut cached: Option<MapChunk> = None;
        let mut cached_pos = None;
        let mut generated_positions = Vec::new();
        let mut generated_indices = FxHashMap::default();
        let mut generated_pixels = Vec::new();
        for row in 0..MAP_SIZE {
            for column in 0..MAP_SIZE {
                let sample = BlockPos::new(
                    start_x.wrapping_add(column as i32).wrapping_mul(scale),
                    world.sea_level(),
                    start_z.wrapping_add(row as i32).wrapping_mul(scale),
                );
                let quart = fuzzed_biome_quart_position(seed, sample);
                let chunk_pos = ChunkPos::new(
                    quart.x >> QUARTS_PER_SECTION_BITS,
                    quart.z >> QUARTS_PER_SECTION_BITS,
                );
                if cached_pos != Some(chunk_pos) {
                    cached = MapChunk::load_if_present(world, chunk_pos, ChunkStatus::Biomes).await;
                    cached_pos = Some(chunk_pos);
                }
                let biome = if let Some(chunk) = cached
                    .as_ref()
                    .and_then(|chunk| chunk.holder.try_chunk(ChunkStatus::Biomes))
                {
                    let sections = &chunk.sections().sections;
                    if sections.is_empty() {
                        return Err(io::Error::other("map biome chunk has no sections"));
                    }
                    let relative_y = (i64::from(quart.y)
                        - i64::from(chunk.min_y() >> BLOCKS_PER_QUART_BITS))
                    .clamp(0, sections.len() as i64 * BiomePalette::SIZE as i64 - 1)
                        as usize;
                    let biome_id = sections[relative_y / BiomePalette::SIZE].read().biomes.get(
                        (quart.x & (BiomePalette::SIZE as i32 - 1)) as usize,
                        relative_y & (BiomePalette::SIZE - 1),
                        (quart.z & (BiomePalette::SIZE as i32 - 1)) as usize,
                    );
                    let Some(biome) = REGISTRY.biomes.by_id(usize::from(biome_id)) else {
                        return Err(io::Error::other(format!(
                            "map sample has unregistered biome {biome_id}"
                        )));
                    };
                    biome
                } else {
                    let index = *generated_indices.entry(quart).or_insert_with(|| {
                        let index = generated_positions.len();
                        generated_positions.push(quart);
                        index
                    });
                    generated_pixels.push((row * MAP_SIZE + column, index));
                    continue;
                };
                watery[row * MAP_SIZE + column] = REGISTRY
                    .biomes
                    .is_in_tag(biome, &vanilla_biome_tags::BiomeTag::WATER_ON_MAP_OUTLINES);
            }
        }
        let generated = world
            .chunk_map
            .world_gen_context
            .generator
            .noise_biomes(&generated_positions);
        for (pixel, index) in generated_pixels {
            watery[pixel] = REGISTRY.biomes.is_in_tag(
                generated[index],
                &vanilla_biome_tags::BiomeTag::WATER_ON_MAP_OUTLINES,
            );
        }
        data.lock().draw_biome_preview(&watery);
        Ok(())
    }

    pub(super) fn draw_biome_preview(&mut self, watery: &[bool; MAP_SIZE * MAP_SIZE]) {
        for x in 1..MAP_SIZE as i32 - 1 {
            for z in 1..MAP_SIZE as i32 - 1 {
                let mut water_count = 0;
                for dx in -1..=1 {
                    for dz in -1..=1 {
                        if (dx != 0 || dz != 0)
                            && watery[((z + dz) * MAP_SIZE as i32 + x + dx) as usize]
                        {
                            water_count += 1;
                        }
                    }
                }
                let mut brightness = brightness::LOWEST;
                let color = if watery[(z * MAP_SIZE as i32 + x) as usize] {
                    if water_count > OPEN_WATER_NEIGHBOR_THRESHOLD
                        && z % OCEAN_STRIPE_ROW_INTERVAL == 0
                    {
                        brightness = match ((x
                            + (trig::sin(z as f32) * OCEAN_STRIPE_AMPLITUDE) as i32)
                            / OCEAN_STRIPE_WIDTH)
                            % OCEAN_STRIPE_PERIOD
                        {
                            0 | 4 => brightness::LOW,
                            1 | 3 => brightness::NORMAL,
                            2 => brightness::HIGH,
                            _ => brightness::LOWEST,
                        };
                    } else if water_count > OPEN_WATER_NEIGHBOR_THRESHOLD {
                        continue;
                    } else if water_count > NORMAL_WATER_NEIGHBOR_THRESHOLD {
                        brightness = brightness::NORMAL;
                    } else if water_count > LOW_WATER_NEIGHBOR_THRESHOLD {
                        brightness = brightness::LOW;
                    }
                    colors::COLOR_ORANGE
                } else if water_count > 0 {
                    if water_count > NORMAL_COAST_NEIGHBOR_THRESHOLD {
                        brightness = brightness::NORMAL;
                    }
                    colors::COLOR_BROWN
                } else {
                    continue;
                };
                self.set_color(x as u8, z as u8, color.packed_id(brightness));
            }
        }
    }
}
