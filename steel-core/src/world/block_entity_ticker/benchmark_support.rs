//! Fixtures for block-entity ticker benchmarks.

use std::{slice, sync::Arc};

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::{vanilla_blocks, vanilla_dimension_types};
use steel_utils::types::{Difficulty, GameType, UpdateFlags};
use steel_utils::{BlockPos, ChunkPos, Identifier};
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};
use toml::map::Map;

use crate::bootstrap::init_globals_once;
use crate::chunk::Chunk;
use crate::chunk::chunk_holder::{ChunkHolder, TickingReadiness};
use crate::chunk::chunk_ticket_manager::ChunkTicketLevel;
use crate::chunk::section::{ChunkSection, Sections};
use crate::chunk::status::ChunkStatus;
use crate::level_data::WorldGenerationSettings;
use crate::world::{World, WorldConfig, WorldStorageConfig};
use crate::worldgen::{ChunkGeneratorType, EmptyChunkGenerator};

/// Prepared world containing one chunk filled with locked, sleeping hoppers.
pub struct SleepingHopperBenchmark {
    world: Arc<World>,
    _runtime: Arc<Runtime>,
}

impl SleepingHopperBenchmark {
    /// Number of hoppers ticked by each benchmark iteration.
    pub const HOPPER_COUNT: u64 = 4_096;

    /// Builds the benchmark world and advances its hoppers into their steady sleeping state.
    ///
    /// # Panics
    ///
    /// Panics if the isolated runtime or world cannot be created, or if the benchmark fixture
    /// cannot place and register exactly [`Self::HOPPER_COUNT`] locked hoppers.
    #[must_use]
    pub fn new() -> Self {
        init_globals_once();

        let runtime = Arc::new(
            RuntimeBuilder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .expect("hopper benchmark runtime should start"),
        );
        let generation_pool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .thread_name(|index| format!("hopper-benchmark-generation-{index}"))
                .build()
                .expect("hopper benchmark generation pool should start"),
        );
        let generator = Arc::new(ChunkGeneratorType::Empty(EmptyChunkGenerator::new()));
        let generator_config = toml::Value::Table(Map::new());
        let generation_settings = WorldGenerationSettings::from_generator_config(
            Identifier::vanilla_static("empty"),
            &generator_config,
            vanilla_dimension_types::OVERWORLD.key.clone(),
            vanilla_dimension_types::OVERWORLD.min_y,
            vanilla_dimension_types::OVERWORLD.height,
        );
        let world = runtime
            .block_on(World::new_with_config(
                Arc::clone(&runtime),
                Identifier::new_static("bench", "hopper_sleep"),
                &vanilla_dimension_types::OVERWORLD,
                0,
                WorldConfig {
                    storage: WorldStorageConfig::RamOnly,
                    level_data_path: None,
                    generator,
                    generation_settings,
                    view_distance: 2,
                    simulation_distance: 2,
                    max_chained_neighbor_updates: 1_000_000,
                    compression: None,
                    is_flat: false,
                    sea_level: 63,
                    default_gamemode: GameType::Survival,
                    difficulty: Difficulty::Normal,
                },
                generation_pool,
            ))
            .expect("hopper benchmark world should initialize");

        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let origin = BlockPos::new(0, 64, 0);
        let locked_hopper = vanilla_blocks::HOPPER
            .default_state()
            .set_value(&BlockStateProperties::ENABLED, false);
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    assert!(world.set_block(
                        BlockPos::new(x, origin.y() + y, z),
                        locked_hopper,
                        UpdateFlags::UPDATE_NONE | UpdateFlags::UPDATE_SKIP_ON_PLACE,
                    ));
                }
            }
        }
        assert_eq!(
            world.block_entity_tickers.registered_len(),
            Self::HOPPER_COUNT as usize
        );

        // The first tick observes that each locked hopper has no work and puts it to sleep.
        world.block_entity_tickers.tick(&world, true);

        Self {
            world,
            _runtime: runtime,
        }
    }

    /// Runs only the world's block-entity ticker phase once.
    pub fn tick(&self) {
        self.world.block_entity_tickers.tick(&self.world, true);
    }
}

impl Default for SleepingHopperBenchmark {
    fn default() -> Self {
        Self::new()
    }
}

fn insert_ready_full_chunk(world: &Arc<World>, pos: ChunkPos) {
    let min_y = world.get_min_y();
    let height = world.get_height();
    let sections = (0..height / 16)
        .map(|_| ChunkSection::new_empty())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let chunk = Chunk::new(
        Sections::from_owned(sections),
        pos,
        min_y,
        height,
        Arc::downgrade(world),
    );
    let _ = chunk.promote_to_full();
    let holder = Arc::new(ChunkHolder::new(
        pos,
        ChunkTicketLevel::BLOCK_TICKING_CHUNK,
        Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK),
        min_y,
        height,
    ));
    holder.insert_chunk(chunk, ChunkStatus::Full);
    assert_eq!(
        holder.transition_ticking_readiness(TickingReadiness::BlockTicking),
        Some(TickingReadiness::Unready)
    );
    let _ = world.chunk_map.chunks.insert_sync(pos, Arc::clone(&holder));
    world.on_entity_chunk_loaded(pos);
    world.update_entity_chunk_visibility(pos, holder.entity_visibility());
    world
        .chunk_map
        .activate_block_entities(slice::from_ref(&holder));
    world.chunk_map.rebuild_ticking_chunk_snapshot();
}
