use std::sync::Arc;

use futures::executor::block_on;
use rayon::ThreadPoolBuilder;
use serde_json::Value;
use steel_registry::{REGISTRY, vanilla_dimension_types, vanilla_timelines, vanilla_world_clocks};
use steel_utils::locks::SyncMutex;
use steel_utils::types::{Difficulty, GameType};
use steel_utils::{ChunkPos, Identifier};
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};
use toml::map::Map;
use uuid::Uuid;

use crate::bootstrap::init_globals_once;
use crate::chunk::chunk_holder::ChunkHolder;
use crate::chunk::chunk_ticket_manager::ChunkTicketLevel;
use crate::chunk::status::ChunkStatus;
use crate::level_data::WorldGenerationSettings;
use crate::world::{World, WorldConfig, WorldGameTickTimings, WorldStorageConfig};
use crate::worldgen::{ChunkGeneratorType, EmptyChunkGenerator};

use super::{TestHarnessError, TestPlayer};

#[derive(Clone, Copy)]
enum RequiredReadiness {
    Block,
    Entity,
}

impl RequiredReadiness {
    const fn ticket_level(self) -> ChunkTicketLevel {
        match self {
            Self::Block => ChunkTicketLevel::BLOCK_TICKING_CHUNK,
            Self::Entity => ChunkTicketLevel::ENTITY_TICKING_CHUNK,
        }
    }

    const fn halo_radius(self) -> u8 {
        match self {
            Self::Block => 1,
            Self::Entity => 2,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Entity => "entity",
        }
    }

    fn is_satisfied_by(self, holder: &ChunkHolder) -> bool {
        let readiness = holder.ticking_readiness_snapshot();
        match self {
            Self::Block => readiness.is_block_ticking(),
            Self::Entity => readiness.is_entity_ticking(),
        }
    }
}

/// A seed-zero, RAM-only Overworld that runs production Steel gameplay code.
pub struct InMemoryWorld {
    world: Arc<World>,
    _runtime: Arc<Runtime>,
    current_tick: u64,
    chunk_mutation: SyncMutex<()>,
    player_mutation: SyncMutex<()>,
}

impl InMemoryWorld {
    /// Creates an empty Overworld with seed zero and deterministic single-thread resources.
    ///
    /// # Errors
    ///
    /// Returns an error if the runtime, generation pool, or Steel world cannot initialize.
    pub fn new() -> Result<Self, TestHarnessError> {
        init_globals_once();

        let runtime = Arc::new(
            RuntimeBuilder::new_multi_thread()
                .worker_threads(1)
                .thread_name("steel-test-harness-runtime")
                .enable_all()
                .build()
                .map_err(TestHarnessError::Runtime)?,
        );
        let generation_pool = Arc::new(
            ThreadPoolBuilder::new()
                .num_threads(1)
                .thread_name(|index| format!("steel-test-harness-{index}"))
                .build()
                .map_err(TestHarnessError::GenerationPool)?,
        );
        let generator = Arc::new(ChunkGeneratorType::Empty(EmptyChunkGenerator::new()));
        let generator_config = toml::Value::Table(Map::new());
        let generation_settings = WorldGenerationSettings::from_generator_config(
            Identifier::new_static("steel", "empty"),
            &generator_config,
            vanilla_dimension_types::OVERWORLD.key.clone(),
            vanilla_dimension_types::OVERWORLD.min_y,
            vanilla_dimension_types::OVERWORLD.height,
        );
        let config = WorldConfig {
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
        };
        let world_runtime = Arc::clone(&runtime);
        let world_task = runtime.spawn(async move {
            World::new_with_config(
                world_runtime,
                Identifier::new_static("steel", "test_harness"),
                &vanilla_dimension_types::OVERWORLD,
                0,
                config,
                generation_pool,
            )
            .await
        });
        let world = block_on(world_task)
            .map_err(TestHarnessError::WorldTask)?
            .map_err(TestHarnessError::World)?;

        Ok(Self {
            world,
            _runtime: runtime,
            current_tick: 0,
            chunk_mutation: SyncMutex::new(()),
            player_mutation: SyncMutex::new(()),
        })
    }

    /// Returns the production world used for block, entity, and player operations.
    ///
    /// Adapters must serialize world mutations with [`Self::tick_once`], just as Steel's
    /// server loop serializes its world-mutation phase.
    #[must_use]
    pub const fn world(&self) -> &Arc<World> {
        &self.world
    }

    /// Makes an empty 3x3 Full halo synchronously available around a block-ticking center.
    ///
    /// Newly created halo chunks remain load-only. The requested center receives block simulation
    /// and confirmed `BlockTicking` readiness through Steel's normal Full publication lifecycle.
    ///
    /// # Errors
    ///
    /// Returns an error if an externally inserted chunk is incomplete or readiness cannot
    /// be published.
    pub fn ensure_chunk(&self, pos: ChunkPos) -> Result<(), TestHarnessError> {
        self.ensure_chunk_with_readiness(pos, RequiredReadiness::Block)
    }

    /// Makes an empty 5x5 Full halo synchronously available around an entity-ticking center.
    ///
    /// The halo remains load-only. The requested center receives entity simulation and confirmed
    /// `EntityTicking` readiness through Steel's normal Full publication lifecycle.
    ///
    /// # Errors
    ///
    /// Returns an error if an existing chunk is incomplete or the halo cannot be published.
    pub fn ensure_entity_chunk(&self, pos: ChunkPos) -> Result<(), TestHarnessError> {
        self.ensure_chunk_with_readiness(pos, RequiredReadiness::Entity)
    }

    /// Runs exactly one normal production game tick.
    ///
    /// Timed chunk tickets advance before [`World::tick_game`], matching the server tick worker.
    /// The first call runs tick one.
    ///
    /// # Errors
    ///
    /// Returns an error if the harness tick counter is exhausted.
    pub fn tick_once(&mut self) -> Result<WorldGameTickTimings, TestHarnessError> {
        let next_tick = self
            .current_tick
            .checked_add(1)
            .ok_or(TestHarnessError::TickOverflow)?;
        self.world.chunk_map.tick_timed_tickets();
        let timings = self.world.tick_game(next_tick, true);
        self.current_tick = next_tick;
        Ok(timings)
    }

    /// Returns the last completed harness tick, starting at zero.
    #[must_use]
    pub const fn current_tick(&self) -> u64 {
        self.current_tick
    }

    /// Returns the current tick inside the 24,000-tick Overworld day.
    ///
    /// # Errors
    ///
    /// Returns an error if Steel's Overworld clock is missing or contains invalid state.
    pub fn daytime(&self) -> Result<u32, TestHarnessError> {
        let total = self
            .world
            .clock_total_ticks(&vanilla_world_clocks::OVERWORLD)
            .ok_or(TestHarnessError::MissingOverworldClock)?;
        let period = day_period()?;
        u32::try_from(total.rem_euclid(i64::from(period)))
            .map_err(|_| TestHarnessError::InvalidStoredDaytime { value: total })
    }

    /// Sets the Overworld clock's absolute total to one tick within the day period.
    ///
    /// # Errors
    ///
    /// Returns an error for values outside the vanilla day or if the clock is unavailable.
    pub fn set_daytime(&self, value: u32) -> Result<(), TestHarnessError> {
        let period = day_period()?;
        if value >= period {
            return Err(TestHarnessError::InvalidDaytime {
                value,
                max: period - 1,
            });
        }
        self.world
            .set_clock_total_ticks(&vanilla_world_clocks::OVERWORLD, i64::from(value))
            .ok_or(TestHarnessError::MissingOverworldClock)
    }

    /// Sets a registered game rule from its JSON representation.
    ///
    /// # Errors
    ///
    /// Returns an error when the key is unknown or the value has the wrong type or range.
    pub fn set_game_rule(&self, key: &Identifier, value: &Value) -> Result<(), TestHarnessError> {
        let rule = REGISTRY
            .game_rules
            .by_key(key)
            .ok_or_else(|| TestHarnessError::UnknownGameRule { key: key.clone() })?;
        let parsed = rule.deserialize_erased_value(value).ok_or_else(|| {
            TestHarnessError::InvalidGameRuleValue {
                key: key.clone(),
                value: value.clone(),
            }
        })?;
        if self.world.set_erased_game_rule(rule, parsed) {
            Ok(())
        } else {
            Err(TestHarnessError::InvalidGameRuleValue {
                key: key.clone(),
                value: value.clone(),
            })
        }
    }

    /// Creates a production player and attaches it through Steel's world lifecycle.
    ///
    /// The player's starting chunk is made entity-ticking before registration. Entity-ticking
    /// is stronger than the `BlockTicking` guarantee exposed by [`Self::ensure_chunk`].
    ///
    /// # Errors
    ///
    /// Returns an error for invalid identity data, duplicate IDs, or lifecycle rejection.
    pub fn create_player(
        &self,
        uuid: Uuid,
        name: impl Into<String>,
        entity_id: i32,
    ) -> Result<TestPlayer, TestHarnessError> {
        self.ensure_entity_chunk(ChunkPos::new(0, 0))?;
        let _guard = self.player_mutation.lock();
        TestPlayer::attach(&self.world, uuid, name.into(), entity_id)
    }

    fn ensure_chunk_with_readiness(
        &self,
        pos: ChunkPos,
        required: RequiredReadiness,
    ) -> Result<(), TestHarnessError> {
        let _guard = self.chunk_mutation.lock();
        let level = required.ticket_level();
        let holder = self.world.chunk_map.install_test_harness_full_halo(
            pos,
            required.halo_radius(),
            level,
        )?;
        Self::verify_chunk_readiness(pos, &holder, required)
    }

    fn verify_chunk_readiness(
        pos: ChunkPos,
        holder: &ChunkHolder,
        required: RequiredReadiness,
    ) -> Result<(), TestHarnessError> {
        if holder.published_status() == Some(ChunkStatus::Full) && required.is_satisfied_by(holder)
        {
            Ok(())
        } else {
            Err(TestHarnessError::ChunkNotTicking {
                pos,
                required: required.label(),
            })
        }
    }
}

impl Drop for InMemoryWorld {
    fn drop(&mut self) {
        let mut players = Vec::new();
        self.world.players.iter_players(|_, player| {
            players.push(Arc::clone(player));
            true
        });
        for player in players {
            self.world.remove_player_for_world_change(&player);
        }

        self.world.chunk_map.stop_generation_refill_loop();
        self.world.chunk_map.task_tracker.close();
        block_on(self.world.chunk_map.task_tracker.wait());
    }
}

fn day_period() -> Result<u32, TestHarnessError> {
    let Some(period) = vanilla_timelines::DAY.period_ticks else {
        return Err(TestHarnessError::MissingOverworldClock);
    };
    u32::try_from(period)
        .ok()
        .filter(|period| *period > 0)
        .ok_or(TestHarnessError::MissingOverworldClock)
}
