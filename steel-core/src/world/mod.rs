//! This module contains the `World` struct, which represents a world.

use std::{
    io, mem,
    path::Path,
    sync::{
        Arc, LazyLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crate::chunk::chunk_access::{ChunkAccess, ChunkStatus};
use crate::chunk::chunk_ticket_manager::{PersistentChunkTickets, TimedChunkTickets};
use crate::chunk::gameplay_chunk_lookup_cache::GameplayChunkLookupCacheScope;
use crate::chunk::level_chunk::{LevelChunk, LevelChunkBlockSetResult};
use crate::chunk::light::{
    LightLayer, LightSectionEmptinessChange, MAX_LIGHT_LEVEL, has_different_light_properties,
};
use crate::poi::OccupationStatus;
use crate::portal::WorldChangeRequest;
use crate::world::game_event::{
    GameEventContext, GameEventDispatcher, GameEventListenerCount, GameEventListenerStorage,
    SharedGameEventListener,
};
use crate::{chunk::chunk_map::ChunkMapGameTickTimings, world::weather::Weather};
use steel_utils::saved_data::{SavedDataManager, names as saved_data_names};

use glam::DVec3;
use sha2::{Digest, Sha256};
use steel_protocol::packets::game::{
    CBlockDestruction, CChangeDifficulty, CGameEvent, CInitializeBorder, CLevelEvent,
    CLevelParticles, CPlayerChat, CSetBorderCenter, CSetBorderLerpSize, CSetBorderSize,
    CSetBorderWarningDelay, CSetBorderWarningDistance, CSetEntityData, CSetEntityLink,
    CSetEquipment, CSound, CSystemChat, CUpdateAttributes, GameEventType, SoundSource,
};
use steel_protocol::utils::ConnectionProtocol;
use steel_protocol::{
    packet_traits::{ClientPacket, CompressionInfo, EncodedPacket},
    packets::game::CSetTime,
};

use rustc_hash::FxHashSet;
use simdnbt::owned::NbtCompound;
use steel_registry::biome::{BiomeRef, TemperatureModifier};
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{Axis, BlockStateProperties, Direction};
use steel_registry::blocks::shapes::{
    BooleanOp, OffsetVoxelShape, SupportType, VoxelShape, is_offset_face_full, is_shape_full_block,
    join_is_not_empty,
};
use steel_registry::fluid::{FluidRef, FluidState};
use steel_registry::game_events::GameEventRef;
use steel_registry::game_rules::{ErasedGameRuleRef, GameRule, GameRuleValue, GameRuleValueType};
use steel_registry::item_stack::ItemStack;
use steel_registry::level_events;
use steel_registry::loot_table::LootContext;
use steel_registry::particle_type::ParticleData;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_game_rules::{
    BLOCK_DROPS, GLOBAL_SOUND_EVENTS, PLAYERS_NETHER_PORTAL_DEFAULT_DELAY, RANDOM_TICK_SPEED,
};
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt, dimension_type::DimensionTypeRef};
use steel_registry::{block_entity_type::BlockEntityTypeRef, vanilla_dimension_types};
use steel_registry::{
    blocks::BlockRef, vanilla_game_rules::ADVANCE_TIME, vanilla_game_rules::ADVANCE_WEATHER,
};
use steel_registry::{vanilla_blocks, vanilla_entities, vanilla_game_events, vanilla_poi_types};
use steel_utils::block_util::FoundRectangle;
use steel_utils::{
    Downcast as _,
    locks::{SyncMutex, SyncRwLock},
    random::{Random as _, RandomSource, legacy_random::LegacyRandom},
};
use steel_worldgen::{biomes::obfuscate_biome_seed, noise::PerlinSimplexNoise};

use steel_utils::{
    BlockLocalAabb, BlockPos, BlockStateId, ChunkPos, Identifier, PackedBlockPos, SectionPos,
    WorldAabb,
    types::{Difficulty, GameType, UpdateFlags},
};
use tokio::{runtime::Runtime, time::Instant};

use crate::{
    ChunkMap,
    behavior::{BLOCK_BEHAVIORS, BlockCollisionContext, BlockLootContext, FLUID_BEHAVIORS},
    block_entity::{BlockEntity, SharedBlockEntity, entities::EndGatewayBlockEntity},
    chunk::{heightmap::HeightmapType, player_chunk_view::PlayerChunkView},
    chunk_saver::{ChunkStorage, RamOnlyStorage, RegionManager},
    entity::{
        AddEntityError, Entity, EntityChangeSenders, EntityChunkCallback, EntityLifecycleChanges,
        EntityMovementSyncPacket, EntityOwnership, EntityTracker, EntityVisibility,
        InactiveEntityCallback, MobEffectSyncPacket, RemovalReason, SharedEntity,
        WorldEntityManager,
        entities::{ExperienceOrbEntity, ItemEntity},
        entity_loot_ref,
    },
    fluid::{FluidStateExt as _, fluid_state_to_block},
    level_data::{LevelDataManager, RespawnData, WorldGenerationSettings},
    player::{LastSeen, Player, connection::NetworkConnection},
    poi::PointOfInterestStorage,
};

mod block_entity_ticker;
mod block_event;
mod block_updates;
mod border;
mod broadcasts;
pub(crate) mod clock;
mod entity_management;
mod environment;
mod events;
/// Vanilla game-event contexts, listeners, and dispatch storage.
pub mod game_event;
mod level_effects;
mod level_reader;
mod player_index;
pub(crate) mod player_spawn_finder;
mod portals;
mod properties;
mod raycast;
mod redstone;
mod signal_getter;
mod spawn;
pub mod tick_scheduler;
mod weather;
mod world_entities;

#[cfg(test)]
mod tests;

pub use crate::config::WorldStorageConfig;
use crate::worldgen::generators::vanilla::fuzzed_biome_at_block;
use crate::worldgen::{ChunkGenerator, ChunkGeneratorType};
use block_event::BlockEventQueue;
use block_updates::CollectingNeighborUpdater;
pub use border::WorldBorderError;
use border::{WorldBorder, WorldBorderSnapshot};
use entity_management::NavigatingMobTracker;
#[cfg(test)]
use entity_management::nearest_player_distance_in_range;
pub use level_reader::{LevelAccessor, LevelReader, ScheduledTickAccess};
pub use player_index::{PlayerAreaMap, PlayerMap};
pub use raycast::{ClipBlockShape, ClipFluid, ClipHitResult, RaytraceAction};
pub use signal_getter::{SignalGetter, SignalQueryContext};
pub(crate) use signal_getter::{
    get_best_neighbor_signal, get_control_input_signal, get_signal, is_redstone_conductor,
};
pub use tick_scheduler::ScheduledTick;

#[cfg(test)]
use level_effects::sound_is_within_range;
#[cfg(test)]
use portals::{
    closest_portal_candidate, nether_portal_creation_scan_origin, nether_portal_frame_offset_pos,
};

const fn initialize_border_packet(snapshot: WorldBorderSnapshot) -> CInitializeBorder {
    CInitializeBorder {
        new_center_x: snapshot.center_x,
        new_center_z: snapshot.center_z,
        old_size: snapshot.old_size,
        new_size: snapshot.new_size,
        lerp_time: snapshot.lerp_time,
        new_absolute_max_size: snapshot.absolute_max_size,
        warning_blocks: snapshot.warning_blocks,
        warning_time: snapshot.warning_time,
    }
}

/// Timing information for a world game tick.
#[derive(Debug)]
pub struct WorldGameTickTimings {
    /// Total time for this world's tick.
    pub elapsed: Duration,
    /// Chunk map game tick timings.
    pub chunk_map: ChunkMapGameTickTimings,
    /// Time spent ticking entities.
    pub entity_tick: Duration,
}

/// Result of replacing a block only when its current state still matches a prior read.
#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConditionalBlockSetResult {
    /// The expected state was claimed and replaced.
    Changed,
    /// The expected state already equals the requested state, so no callbacks ran.
    Unchanged,
    /// The current state did not match the caller's expected state.
    Stale(BlockStateId),
    /// The position is invalid, the chunk is unavailable, or the update limit was exhausted.
    Unavailable,
}

/// Configuration for creating a new world.
#[derive(Clone)]
pub struct WorldConfig {
    /// Storage configuration for chunk persistence.
    pub storage: WorldStorageConfig,
    /// Directory for level data. `None` means level data is ephemeral.
    pub level_data_path: Option<String>,
    /// World generator.
    pub generator: Arc<ChunkGeneratorType>,
    /// Generator metadata persisted for startup compatibility checks.
    pub generation_settings: WorldGenerationSettings,
    /// Server view distance (maximum chunk radius).
    pub view_distance: u8,
    /// Server simulation distance.
    pub simulation_distance: u8,
    /// Maximum queued neighbor-update tasks in one chained run; negative means unlimited.
    pub max_chained_neighbor_updates: i32,
    /// Compression settings for encoding broadcast packets.
    pub compression: Option<CompressionInfo>,
    /// Whether the world should be marked as flat in login/respawn packets.
    pub is_flat: bool,
    /// Sea level sent in login/respawn packets.
    pub sea_level: i32,
    /// Default game mode for first-visit player data.
    pub default_gamemode: GameType,
    /// Difficulty used when creating new level data.
    pub difficulty: Difficulty,
}

/// A struct that represents a world.
pub struct World {
    /// The chunk map of the world.
    pub chunk_map: Arc<ChunkMap>,
    /// All players in the world with dual indexing by UUID and entity ID.
    pub players: PlayerMap,
    /// Spatial index for player proximity queries.
    pub player_area_map: PlayerAreaMap,
    /// Loaded world identifier (`domain:world`).
    pub key: Identifier,
    /// Vanilla dimension type for this loaded world.
    ///
    /// Vanilla often calls loaded worlds "dimensions". In Steel, `World` is the
    /// loaded world instance and `dimension_type` is the vanilla registry entry
    /// controlling height, skylight, ceiling, water evaporation, etc.
    pub dimension_type: DimensionTypeRef,
    /// Level data manager for persistent world state.
    pub level_data: SyncRwLock<LevelDataManager>,
    /// Per-world saved data storage.
    pub(crate) saved_data: SavedDataManager,
    /// Runtime world border state.
    world_border: SyncMutex<WorldBorder>,
    /// Server view distance (maximum chunk radius).
    pub view_distance: u8,
    /// Server simulation distance.
    pub simulation_distance: u8,
    /// Compression settings for encoding broadcast packets.
    pub compression: Option<CompressionInfo>,
    /// Whether the world should be marked as flat in login/respawn packets.
    pub is_flat: bool,
    /// Sea level sent in login/respawn packets.
    pub sea_level: i32,
    /// Default game mode for first-visit player data.
    pub default_gamemode: GameType,
    /// Whether the tick rate is running normally (not frozen/paused).
    /// When false, movement validation checks are skipped.
    tick_runs_normally: AtomicBool,
    /// Whether vanilla's scheduled/chunk/block-event tick phase is active.
    handling_tick: AtomicBool,
    /// Ordered, duplicate-suppressing server block events awaiting execution.
    block_events: SyncMutex<BlockEventQueue>,
    /// Vanilla collecting neighbor updater shared by all live block mutations.
    neighbor_updater: CollectingNeighborUpdater,
    /// Central runtime entity ownership and lookup.
    entity_manager: WorldEntityManager,
    /// World-global ordered block-entity ticker phase.
    block_entity_tickers: block_entity_ticker::WorldBlockEntityTickers,
    /// Physical entries retained by this world's chunk-owned game-event registries.
    game_event_listener_count: Arc<GameEventListenerCount>,
    /// Entity tracker for managing which players can see which entities.
    entity_tracker: EntityTracker,
    /// Runtime IDs for pathfinder mobs currently visible to the active world.
    navigating_mobs: NavigatingMobTracker,
    /// Weather Data needed for animating starting and stopping of rain clientside
    pub weather: SyncMutex<Weather>,
    /// Per-level recent toggle history used by vanilla redstone-torch burnout.
    redstone_torch_toggles: SyncMutex<redstone::RedstoneTorchToggleTracker>,
    /// World registration and sparse head index for chunk-owned scheduled ticks.
    scheduled_ticks: tick_scheduler::WorldTickScheduler,
    /// Published block batch used by `willTickThisTick` queries during callbacks.
    scheduled_block_ticks_this_tick:
        SyncMutex<Option<Arc<tick_scheduler::ScheduledTickRunBatch<BlockRef>>>>,
    /// Published fluid batch used by `willTickThisTick` queries during callbacks.
    scheduled_fluid_ticks_this_tick:
        SyncMutex<Option<Arc<tick_scheduler::ScheduledTickRunBatch<FluidRef>>>>,
    /// Point of interest storage for efficient spatial queries of special blocks.
    pub poi_storage: SyncMutex<PointOfInterestStorage>,
    /// World-change requests queued by world-local ticks for server safe-point processing.
    pending_world_changes: SyncMutex<Vec<(SharedEntity, WorldChangeRequest)>>,
}

impl World {
    /// Creates a new world with custom configuration.
    ///
    /// This allows specifying storage backend (disk or RAM-only) and other options.
    /// Uses `Arc::new_cyclic` to create a cyclic reference between
    /// the World and its `ChunkMap`'s `WorldGenContext`.
    ///
    /// # Arguments
    /// * `chunk_runtime` - The Tokio runtime for chunk operations
    /// * `dimension_type` - Vanilla dimension type (overworld, nether, end)
    /// * `seed` - The world seed
    /// * `config` - World configuration including storage options
    pub async fn new_with_config(
        chunk_runtime: Arc<Runtime>,
        key: Identifier,
        dimension_type: DimensionTypeRef,
        seed: i64,
        config: WorldConfig,
        generation_pool: Arc<rayon::ThreadPool>,
    ) -> io::Result<Arc<Self>> {
        let chunk_encoding_pool = Arc::clone(&generation_pool);
        Self::new_with_config_and_encoding_pool(
            chunk_runtime,
            key,
            dimension_type,
            seed,
            config,
            generation_pool,
            chunk_encoding_pool,
        )
        .await
    }

    pub(crate) async fn new_with_config_and_encoding_pool(
        chunk_runtime: Arc<Runtime>,
        key: Identifier,
        dimension_type: DimensionTypeRef,
        seed: i64,
        config: WorldConfig,
        generation_pool: Arc<rayon::ThreadPool>,
        chunk_encoding_pool: Arc<rayon::ThreadPool>,
    ) -> io::Result<Arc<Self>> {
        let view_distance = config.view_distance;
        let simulation_distance = config.simulation_distance;
        let max_chained_neighbor_updates = config.max_chained_neighbor_updates;
        let compression = config.compression;
        let is_flat = config.is_flat;
        let sea_level = config.sea_level;
        let default_gamemode = config.default_gamemode;
        // Create storage backend based on config
        let storage: Arc<ChunkStorage> = match &config.storage {
            WorldStorageConfig::Disk { path } => {
                Arc::new(ChunkStorage::Disk(RegionManager::new(path.clone())))
            }
            WorldStorageConfig::RamOnly => {
                Arc::new(ChunkStorage::RamOnly(RamOnlyStorage::empty_world()))
            }
        };

        // Create or skip level data based on config

        let path = config.level_data_path.as_deref().map(Path::new);
        let saved_data = SavedDataManager::new(path);
        let mut level_data =
            LevelDataManager::new(path, seed, config.difficulty, config.generation_settings)
                .await?;
        if level_data.is_dirty() {
            level_data.save().await?;
        }
        let persistent_chunk_tickets: PersistentChunkTickets = saved_data
            .load_or_default(saved_data_names::CHUNK_TICKETS)
            .await?;
        let timed_chunk_tickets = TimedChunkTickets::from_persistent(persistent_chunk_tickets);
        let world_border = WorldBorder::new(level_data.data().world_border)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        // let generator = Arc::new(ChunkGeneratorType::Flat(FlatChunkGenerator::new(
        //     REGISTRY
        //         .blocks
        //         .get_default_state_id(vanilla_blocks::BEDROCK), // Bedrock
        //     REGISTRY.blocks.get_default_state_id(vanilla_blocks::DIRT), // Dirt
        //     REGISTRY
        //         .blocks
        //         .get_default_state_id(vanilla_blocks::GRASS_BLOCK), // Grass Block
        // )));

        let mut weather = Weather::default();
        if level_data.is_raining() {
            weather.rain_level = 1.0;
            if level_data.is_thundering() {
                weather.thunder_level = 1.0;
            }
        }

        Ok(Arc::new_cyclic(|weak_self: &Weak<World>| {
            let chunk_map = Arc::new(ChunkMap::new_with_storage_and_timed_tickets(
                chunk_runtime,
                weak_self.clone(),
                dimension_type,
                sea_level,
                storage,
                config.generator,
                generation_pool,
                chunk_encoding_pool,
                timed_chunk_tickets,
            ));
            chunk_map.start_generation_refill_loop();

            Self {
                chunk_map,
                players: PlayerMap::new(),
                player_area_map: PlayerAreaMap::new(),
                key,
                dimension_type,
                level_data: SyncRwLock::new(level_data),
                saved_data,
                world_border: SyncMutex::new(world_border),
                view_distance,
                simulation_distance,
                compression,
                is_flat,
                sea_level,
                default_gamemode,
                tick_runs_normally: AtomicBool::new(true),
                handling_tick: AtomicBool::new(false),
                block_events: SyncMutex::new(BlockEventQueue::default()),
                neighbor_updater: CollectingNeighborUpdater::new(max_chained_neighbor_updates),
                entity_manager: WorldEntityManager::new(),
                block_entity_tickers: block_entity_ticker::WorldBlockEntityTickers::new(),
                game_event_listener_count: GameEventListenerCount::shared(),
                entity_tracker: EntityTracker::new(),
                navigating_mobs: NavigatingMobTracker::new(),
                weather: SyncMutex::new(weather),
                redstone_torch_toggles: SyncMutex::new(
                    redstone::RedstoneTorchToggleTracker::default(),
                ),
                scheduled_ticks: tick_scheduler::WorldTickScheduler::new(),
                scheduled_block_ticks_this_tick: SyncMutex::new(None),
                scheduled_fluid_ticks_this_tick: SyncMutex::new(None),
                poi_storage: SyncMutex::new(PointOfInterestStorage::new()),
                pending_world_changes: SyncMutex::new(Vec::new()),
            }
        }))
    }

    /// Cleans up the world by saving all chunks.
    #[expect(
        clippy::await_holding_lock,
        reason = "holding the write lock across await is safe here because it only happens during shutdown"
    )]
    pub async fn cleanup(&self, total_saved: &mut usize) {
        self.sync_world_border_to_level_data();
        match self.level_data.write().save().await {
            Ok(()) => log::info!("World {} level data saved successfully", self.key),
            Err(e) => log::error!("Failed to save world level data: {e}"),
        }

        let chunk_tickets = self.chunk_map.persistent_chunk_tickets();
        match self
            .saved_data
            .save(saved_data_names::CHUNK_TICKETS, &chunk_tickets)
            .await
        {
            Ok(()) => log::info!("World {} saved chunk ticket data successfully", self.key),
            Err(e) => log::error!("Failed to save world chunk ticket data: {e}"),
        }

        match self.save_all_chunks().await {
            Ok(count) => *total_saved += count,
            Err(e) => log::error!("Failed to save world chunks: {e}"),
        }
    }

    /// Returns the domain this loaded world belongs to.
    #[must_use]
    pub fn domain(&self) -> &str {
        self.key.namespace.as_ref()
    }

    /// Game tick: weather, time, chunk game tick (broadcasts + random/scheduled ticks),
    /// and player logic (without chunk sending).
    ///
    /// * `tick_count` - The current tick number
    /// * `runs_normally` - Whether game elements (random ticks, entities) should run.
    ///   When false (frozen), only essential operations like chunk loading run.
    #[tracing::instrument(level = "trace", skip(self), name = "world_game_tick")]
    #[expect(
        clippy::too_many_lines,
        reason = "world tick orchestration keeps vanilla subsystem order explicit"
    )]
    pub fn tick_game(
        self: &Arc<Self>,
        tick_count: u64,
        runs_normally: bool,
    ) -> WorldGameTickTimings {
        let world_start = Instant::now();
        let lookup_cache_scope = GameplayChunkLookupCacheScope::enter(&self.chunk_map);
        self.handling_tick.store(true, Ordering::Relaxed);
        self.set_tick_runs_normally(runs_normally);
        if runs_normally {
            self.tick_world_border();
            self.tick_weather();
            self.tick_time();
        }

        let random_tick_speed = self.get_game_rule(&RANDOM_TICK_SPEED) as u32;

        let mut chunk_map_timings =
            self.chunk_map
                .tick_game(self, tick_count, random_tick_speed, runs_normally);

        if runs_normally {
            let _span = tracing::trace_span!("block_events").entered();
            self.run_block_events();
        }

        // Vanilla clears this before ticking entities and block entities.
        self.handling_tick.store(false, Ordering::Relaxed);

        let entity_tick = {
            let _span = tracing::trace_span!("entity_tick").entered();
            let start = Instant::now();
            let dirty_chunks = self
                .entity_manager
                .tick_entities(tick_count as i32, runs_normally);
            for chunk in dirty_chunks {
                self.mark_chunk_dirty(chunk);
            }
            start.elapsed()
        };

        {
            let _span = tracing::trace_span!("block_entities").entered();
            let start = Instant::now();
            self.block_entity_tickers.tick(self, runs_normally);
            chunk_map_timings.tick_block_entities = start.elapsed();
        }

        {
            let _span = tracing::trace_span!("entity_tracker_send_changes").entered();
            self.entity_tracker.send_changes(
                |chunk| self.get_packet_tracking_players(chunk),
                |player_id| self.players.get_by_entity_id(player_id),
                EntityChangeSenders {
                    movement: |entity_id, packet| {
                        self.broadcast_movement_sync_to_entity_trackers(entity_id, packet, None);
                    },
                    self_movement: |player_id, packet| {
                        let Some(encoded) = self.encode_movement_sync_packet(packet) else {
                            return;
                        };
                        let Some(player) = self.players.get_by_entity_id(player_id) else {
                            return;
                        };
                        player.connection.send_encoded(encoded);
                    },
                    entity_data: |entity_id, dirty_entity_data| {
                        let packet = CSetEntityData::new(entity_id, dirty_entity_data);
                        let Ok(encoded) = EncodedPacket::from_bare(
                            packet,
                            self.compression,
                            ConnectionProtocol::Play,
                        ) else {
                            return;
                        };
                        self.broadcast_to_entity_trackers_encoded(entity_id, encoded.clone(), None);
                        if let Some(player) = self.players.get_by_entity_id(entity_id) {
                            player.connection.send_encoded(encoded);
                        }
                    },
                    attributes: |entity_id, dirty_attributes| {
                        let packet = CUpdateAttributes::new(entity_id, dirty_attributes);
                        let Ok(encoded) = EncodedPacket::from_bare(
                            packet,
                            self.compression,
                            ConnectionProtocol::Play,
                        ) else {
                            return;
                        };
                        self.broadcast_to_entity_trackers_encoded(entity_id, encoded.clone(), None);
                        if let Some(player) = self.players.get_by_entity_id(entity_id) {
                            player.connection.send_encoded(encoded);
                        }
                    },
                    mob_effects: |player_id, packet| {
                        let Some(player) = self.players.get_by_entity_id(player_id) else {
                            return;
                        };
                        match packet {
                            MobEffectSyncPacket::Update(packet) => player.send_packet(packet),
                            MobEffectSyncPacket::Remove(packet) => player.send_packet(packet),
                        }
                    },
                    equipment: |entity_id, packet: CSetEquipment| {
                        let Ok(encoded) = EncodedPacket::from_bare(
                            packet,
                            self.compression,
                            ConnectionProtocol::Play,
                        ) else {
                            return;
                        };
                        self.broadcast_to_entity_trackers_encoded(entity_id, encoded, None);
                    },
                    passengers: |player_id, packet| {
                        if let Some(player) = self.players.get_by_entity_id(player_id) {
                            player.send_packet(packet);
                        }
                    },
                    entity_link: |entity_id, packet: CSetEntityLink| {
                        let Ok(encoded) = EncodedPacket::from_bare(
                            packet,
                            self.compression,
                            ConnectionProtocol::Play,
                        ) else {
                            return;
                        };
                        self.broadcast_to_entity_trackers_encoded(entity_id, encoded, None);
                    },
                },
            );
        }

        chunk_map_timings.lookup_cache = lookup_cache_scope.finish();
        WorldGameTickTimings {
            elapsed: world_start.elapsed(),
            chunk_map: chunk_map_timings,
            entity_tick,
        }
    }

    /// Saves all dirty chunks in this world to disk.
    ///
    /// This should be called during graceful shutdown.
    /// Returns the number of chunks saved.
    pub async fn save_all_chunks(&self) -> io::Result<usize> {
        self.chunk_map.save_all_chunks().await
    }
}

impl LevelReader for World {
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        Self::get_block_state(self, pos)
    }

    fn get_block_entity(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        Self::get_block_entity(self, pos)
    }

    fn is_face_sturdy_for(
        &self,
        state: BlockStateId,
        pos: BlockPos,
        direction: Direction,
        support_type: SupportType,
    ) -> bool {
        BLOCK_BEHAVIORS
            .get_behavior(state.get_block())
            .is_face_sturdy(state, self, pos, direction, support_type)
    }

    fn raw_brightness(&self, pos: BlockPos, sky_darkening: u8) -> u8 {
        let sky_light = if self.dimension_type.has_skylight {
            self.light_value_at(LightLayer::Sky, pos)
                .saturating_sub(sky_darkening)
        } else {
            0
        };

        if sky_light == MAX_LIGHT_LEVEL {
            return MAX_LIGHT_LEVEL;
        }

        sky_light.max(self.light_value_at(LightLayer::Block, pos))
    }

    fn can_see_sky(&self, pos: BlockPos) -> bool {
        Self::can_see_sky(self, pos)
    }

    fn ambient_light(&self) -> f32 {
        self.dimension_type.ambient_light
    }

    fn min_y(&self) -> i32 {
        self.get_min_y()
    }

    fn height(&self) -> i32 {
        self.get_height()
    }
}

impl LevelReader for Arc<World> {
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        self.as_ref().get_block_state(pos)
    }

    fn get_block_entity(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        self.as_ref().get_block_entity(pos)
    }

    fn is_face_sturdy_for(
        &self,
        state: BlockStateId,
        pos: BlockPos,
        direction: Direction,
        support_type: SupportType,
    ) -> bool {
        self.as_ref()
            .is_face_sturdy_for(state, pos, direction, support_type)
    }

    fn raw_brightness(&self, pos: BlockPos, sky_darkening: u8) -> u8 {
        self.as_ref().raw_brightness(pos, sky_darkening)
    }

    fn can_see_sky(&self, pos: BlockPos) -> bool {
        self.as_ref().can_see_sky(pos)
    }

    fn ambient_light(&self) -> f32 {
        self.as_ref().ambient_light()
    }

    fn min_y(&self) -> i32 {
        self.as_ref().get_min_y()
    }

    fn height(&self) -> i32 {
        self.as_ref().get_height()
    }
}

impl ScheduledTickAccess for Arc<World> {
    fn fluid_tick_delay(&self, fluid: FluidRef) -> i32 {
        FLUID_BEHAVIORS.get_behavior(fluid).tick_delay(self)
    }

    fn schedule_block_tick_default(&self, pos: BlockPos, block: BlockRef, delay: i32) -> bool {
        self.as_ref().schedule_block_tick_default(pos, block, delay);
        true
    }

    fn has_scheduled_block_tick(&self, pos: BlockPos, block: BlockRef) -> bool {
        self.as_ref().has_scheduled_block_tick(pos, block)
    }

    fn will_tick_block_this_tick(&self, pos: BlockPos, block: BlockRef) -> bool {
        self.as_ref().will_tick_block_this_tick(pos, block)
    }

    fn schedule_fluid_tick_default(&self, pos: BlockPos, fluid: FluidRef, delay: i32) -> bool {
        self.as_ref().schedule_fluid_tick_default(pos, fluid, delay);
        true
    }

    fn will_tick_fluid_this_tick(&self, pos: BlockPos, fluid: FluidRef) -> bool {
        self.as_ref().will_tick_fluid_this_tick(pos, fluid)
    }
}

impl LevelAccessor for Arc<World> {
    fn set_block_state(&self, pos: BlockPos, state: BlockStateId, flags: UpdateFlags) -> bool {
        self.set_block(pos, state, flags)
    }

    fn play_block_sound(
        &self,
        sound: SoundEventRef,
        pos: BlockPos,
        volume: f32,
        pitch: f32,
        exclude: Option<i32>,
    ) {
        self.as_ref()
            .play_block_sound(sound, pos, volume, pitch, exclude);
    }

    fn game_event(&self, event: GameEventRef, pos: BlockPos, context: &GameEventContext<'_>) {
        World::game_event(self, event, pos, context);
    }
}
