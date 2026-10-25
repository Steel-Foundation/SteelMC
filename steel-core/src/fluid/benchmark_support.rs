//! World and chunk fixtures for fluid benchmarks.
//!
//! This is only compiled with the `benchmark-support` feature so benchmarks can
//! build a minimal live world without going through world generation.

use std::sync::Arc;

use steel_registry::dimension_type::DimensionTypeRef;
use steel_registry::vanilla_dimension_types;
use steel_utils::types::{Difficulty, GameType};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, Identifier, PackedSectionBlockPos};
use steel_worldgen::density_functions::overworld::OverworldNoiseSettings;
use tokio::runtime::{Builder, Runtime};
use toml::map::Map;

use crate::bootstrap::init_globals;
use crate::chunk::Chunk;
use crate::chunk::chunk_holder::ChunkHolder;
use crate::chunk::chunk_ticket_manager::ChunkTicketLevel;
use crate::chunk::paletted_container::BlockPalette;
use crate::chunk::section::{ChunkSection, Sections};
use crate::chunk::status::ChunkStatus;
use crate::entity::damage::DamageHistory;
use crate::level_data::{GameTimeSource, WorldGenerationSettings};
use crate::world::{World, WorldConfig, WorldStorageConfig};
use crate::worldgen::{ChunkGeneratorType, EmptyChunkGenerator};

/// A benchmark world plus the resources that must outlive it.
pub struct FluidBenchWorld {
    /// The initialized world; the other fields only keep it alive.
    pub world: Arc<World>,
    _runtime: Arc<Runtime>,
    _generation_pool: Arc<rayon::ThreadPool>,
    _damage_history: Arc<DamageHistory>,
}

/// Creates an empty benchmark world.
///
/// # Panics
///
/// Panics if the runtime, generation pool, or world cannot be initialized.
#[must_use]
pub fn create_world(key: &'static str) -> FluidBenchWorld {
    init_globals();

    let runtime = Arc::new(
        Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("fluid benchmark runtime should start"),
    );
    let generation_pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .thread_name(|index| format!("fluid-bench-world-{index}"))
            .build()
            .expect("fluid benchmark generation pool should start"),
    );
    let damage_history = Arc::new(DamageHistory::default());
    let dimension_type: DimensionTypeRef = &vanilla_dimension_types::OVERWORLD;
    let generator_config = toml::Value::Table(Map::new());
    let generation_settings = WorldGenerationSettings::from_generator_config(
        Identifier::vanilla_static("empty"),
        &generator_config,
        dimension_type.key.clone(),
        dimension_type.min_y,
        dimension_type.height,
    );

    let world = runtime
        .block_on(World::new_with_config(
            Arc::clone(&runtime),
            Identifier::new_static("bench", key),
            dimension_type,
            0,
            WorldConfig {
                damage_history: Arc::clone(&damage_history),
                game_time_source: GameTimeSource::Primary,
                storage: WorldStorageConfig::RamOnly,
                level_data_path: None,
                generator: Arc::new(ChunkGeneratorType::Empty(EmptyChunkGenerator::new())),
                generation_settings,
                view_distance: 2,
                simulation_distance: 2,
                max_chained_neighbor_updates: 1_000_000,
                compression: None,
                is_flat: false,
                sea_level: OverworldNoiseSettings::SEA_LEVEL,
                default_gamemode: GameType::Survival,
                difficulty: Difficulty::Normal,
            },
            Arc::clone(&generation_pool),
        ))
        .expect("fluid benchmark world should initialize");

    FluidBenchWorld {
        world,
        _runtime: runtime,
        _generation_pool: generation_pool,
        _damage_history: damage_history,
    }
}

/// Inserts an empty Full chunk, allowing `populate` to fill its sections before
/// the chunk becomes visible to the world.
pub fn insert_full_chunk(
    world: &Arc<World>,
    pos: ChunkPos,
    populate: impl FnOnce(&Sections, i32),
) -> Arc<ChunkHolder> {
    let min_y = world.get_min_y();
    let height = world.get_height();
    let sections = Sections::from_owned(
        (0..height / BlockPalette::SIZE as i32)
            .map(|_| ChunkSection::new_empty())
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    populate(&sections, min_y);

    let chunk = Chunk::new(sections, pos, min_y, height, Arc::downgrade(world));
    let _ = chunk.promote_to_full();

    let holder = Arc::new(ChunkHolder::new(
        pos,
        ChunkTicketLevel::BLOCK_TICKING_CHUNK,
        None,
        min_y,
        height,
    ));
    holder.insert_chunk(chunk, ChunkStatus::Full);
    world
        .chunk_map
        .insert_benchmark_chunk_holder(pos, Arc::clone(&holder));
    holder
}

/// Writes a block into a chunk section collection before Full promotion.
///
/// `pos` must belong to the chunk that owns `sections`.
pub fn set_block(sections: &Sections, min_y: i32, pos: BlockPos, state: BlockStateId) {
    let relative = PackedSectionBlockPos::from_block_pos(pos);

    sections.set_relative_block(
        usize::from(relative.x()),
        (pos.0.y - min_y) as usize,
        usize::from(relative.z()),
        state,
    );
}
