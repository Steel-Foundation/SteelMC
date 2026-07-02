//! This module contains the `World` struct, which represents a world.

use std::path::Path;
use std::{
    io,
    sync::{
        Arc, LazyLock, Weak,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    time::Duration,
};

use crate::chunk::chunk_access::{ChunkAccess, ChunkStatus};
use crate::chunk::light::{
    LightLayer, LightSectionEmptinessChange, MAX_LIGHT_LEVEL, has_different_light_properties,
};
use crate::world::game_event_context::GameEventContext;
use crate::world::game_event_listener::{GameEventListenerStorage, SharedGameEventListener};
use crate::{chunk::chunk_map::ChunkMapGameTickTimings, world::weather::Weather};

use glam::DVec3;
use sha2::{Digest, Sha256};
use steel_protocol::packets::game::{
    CBlockDestruction, CBlockEvent, CGameEvent, CInitializeBorder, CLevelEvent, CPlayerChat,
    CPlayerInfoUpdate, CSetBorderCenter, CSetBorderLerpSize, CSetBorderSize,
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
use steel_registry::blocks::properties::Direction;
use steel_registry::blocks::shapes::{
    BooleanOp, OffsetVoxelShape, VoxelShape, is_offset_face_full, join_is_not_empty,
};
use steel_registry::fluid::{FluidRef, FluidState};
use steel_registry::game_events::GameEventRef;
use steel_registry::game_rules::{GameRuleRef, GameRuleValue};
use steel_registry::item_stack::ItemStack;
use steel_registry::level_events;
use steel_registry::loot_table::LootContext;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_game_rules::{
    BLOCK_DROPS, PLAYERS_NETHER_PORTAL_DEFAULT_DELAY, RANDOM_TICK_SPEED,
};
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt, dimension_type::DimensionTypeRef};
use steel_registry::{block_entity_type::BlockEntityTypeRef, vanilla_dimension_types};
use steel_registry::{
    blocks::BlockRef, vanilla_game_rules::ADVANCE_TIME, vanilla_game_rules::ADVANCE_WEATHER,
};
use steel_registry::{vanilla_blocks, vanilla_entities, vanilla_game_events};
use steel_utils::{
    locks::{SyncMutex, SyncRwLock},
    random::{RandomSource, legacy_random::LegacyRandom},
};
use steel_worldgen::{biomes::obfuscate_biome_seed, noise::PerlinSimplexNoise};

/// Controls how a block position is treated during a raytrace traversal.
///
/// Returned by the predicate closure passed to [`World::raytrace`].
#[derive(Debug)]
pub enum RaytraceAction {
    /// Skip this block and continue traversal (transparent block).
    Pass,
    /// Test the block's voxel shape for a precise ray intersection.
    CheckShape,
    /// Immediately treat this block as a hit without shape testing.
    ImmediateHit,
}

use steel_utils::{
    BlockLocalAabb, BlockPos, BlockStateId, ChunkPos, Identifier, SectionPos, WorldAabb,
    types::{Difficulty, GameType, UpdateFlags},
};
use tokio::{runtime::Runtime, time::Instant};

use crate::{
    ChunkMap,
    behavior::BlockStateBehaviorExt,
    behavior::{BLOCK_BEHAVIORS, BlockCollisionContext, FLUID_BEHAVIORS},
    block_entity::SharedBlockEntity,
    chunk::{heightmap::HeightmapType, player_chunk_view::PlayerChunkView},
    chunk_saver::{ChunkStorage, RamOnlyStorage, RegionManager},
    entity::{
        AddEntityError, Entity, EntityChangeSenders, EntityChunkCallback, EntityLifecycleChanges,
        EntityMovementSyncPacket, EntityOwnership, EntityTracker, EntityVisibility,
        InactiveEntityCallback, MobEffectSyncPacket, RemovalReason, SharedEntity,
        WorldEntityManager, entities::ItemEntity,
    },
    fluid::{FluidStateExt as _, fluid_state_to_block},
    level_data::{LevelDataManager, RespawnData, WorldBorderData, WorldGenerationSettings},
    player::{LastSeen, Player, connection::NetworkConnection},
    poi::PointOfInterestStorage,
};

static BIOME_TEMPERATURE_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = RandomSource::Legacy(LegacyRandom::from_seed(1234));
    PerlinSimplexNoise::new(&mut random, &[0])
});

static FROZEN_BIOME_TEMPERATURE_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = RandomSource::Legacy(LegacyRandom::from_seed(3456));
    PerlinSimplexNoise::new(&mut random, &[-2, -1, 0])
});

static BIOME_INFO_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = RandomSource::Legacy(LegacyRandom::from_seed(2345));
    PerlinSimplexNoise::new(&mut random, &[0])
});

/// Block shape channel used by vanilla-style world clipping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipBlockShape {
    /// `ClipContext.Block.COLLIDER`
    Collider,
    /// `ClipContext.Block.OUTLINE`
    Outline,
    /// `ClipContext.Block.VISUAL`
    Visual,
    /// `ClipContext.Block.FALLDAMAGE_RESETTING`
    FallDamageResetting {
        /// Whether the clip context entity is a player.
        entity_is_player: bool,
    },
}

/// Fluid shape filter used by vanilla-style world clipping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipFluid {
    /// `ClipContext.Fluid.NONE`
    None,
    /// `ClipContext.Fluid.SOURCE_ONLY`
    SourceOnly,
    /// `ClipContext.Fluid.ANY`
    Any,
    /// `ClipContext.Fluid.WATER`
    Water,
}

/// Result of a vanilla-style world clip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipHitResult {
    /// Exact hit location in world coordinates.
    pub location: DVec3,
    /// Hit face, or the miss direction for misses.
    pub direction: Direction,
    /// Block position containing the hit or miss endpoint.
    pub block_pos: BlockPos,
    /// Whether this result is a miss.
    pub miss: bool,
    /// Whether the ray started inside the hit shape.
    pub inside: bool,
}

impl ClipHitResult {
    /// Returns whether this clip missed all selected block and fluid shapes.
    #[must_use]
    pub const fn is_miss(self) -> bool {
        self.miss
    }
}

mod border;
mod environment;
pub mod game_event_context;
pub mod game_event_listener;
mod level_reader;
mod player_area_map;
mod player_map;
pub(crate) mod player_spawn_finder;
pub mod tick_scheduler;
mod weather;
mod world_entities;

pub use crate::config::WorldStorageConfig;
use crate::worldgen::generators::vanilla::fuzzed_biome_at_block;
use crate::worldgen::{ChunkGenerator, ChunkGeneratorType};
pub use border::WorldBorderError;
use border::{WorldBorder, WorldBorderSnapshot};
pub use level_reader::{LevelAccessor, LevelReader, ScheduledTickAccess};
pub use player_area_map::PlayerAreaMap;
pub use player_map::PlayerMap;
pub use tick_scheduler::ScheduledTick;

/// Generates a random value using triangle distribution.
///
/// Mirrors vanilla's `RandomSource.triangle(mode, deviation)`.
/// Produces values centered around `mode` with a spread of `deviation`.
fn triangle_random(mode: f64, deviation: f64) -> f64 {
    mode + deviation * (rand::random::<f64>() - rand::random::<f64>())
}

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

const fn chunk_min_block_x(pos: ChunkPos) -> i32 {
    pos.0.x << 4
}

const fn chunk_min_block_z(pos: ChunkPos) -> i32 {
    pos.0.y << 4
}

const fn chunk_max_block_x(pos: ChunkPos) -> i32 {
    (pos.0.x << 4) + 15
}

const fn chunk_max_block_z(pos: ChunkPos) -> i32 {
    (pos.0.y << 4) + 15
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

/// Interval in ticks between player info broadcasts (600 ticks = 30 seconds).
/// Matches vanilla `PlayerList.SEND_PLAYER_INFO_INTERVAL`.
const SEND_PLAYER_INFO_INTERVAL: u64 = 600;

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

struct NavigatingMobTracker {
    ids: SyncMutex<FxHashSet<i32>>,
}

impl NavigatingMobTracker {
    fn new() -> Self {
        Self {
            ids: SyncMutex::new(FxHashSet::default()),
        }
    }

    fn track(&self, entity: &SharedEntity) {
        if entity.as_pathfinder_mob().is_some() {
            self.ids.lock().insert(entity.id());
        }
    }

    fn untrack(&self, entity_id: i32) {
        self.ids.lock().remove(&entity_id);
    }

    fn ids(&self) -> Vec<i32> {
        self.ids.lock().iter().copied().collect()
    }
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
    /// Central runtime entity ownership and lookup.
    entity_manager: WorldEntityManager,
    /// Entity tracker for managing which players can see which entities.
    entity_tracker: EntityTracker,
    /// Runtime IDs for pathfinder mobs currently visible to the active world.
    navigating_mobs: NavigatingMobTracker,
    /// Weather Data needed for animating starting and stopping of rain clientside
    pub weather: SyncMutex<Weather>,
    /// Monotonic counter for `sub_tick_order` on scheduled ticks.
    /// Provides stable ordering when multiple ticks fire on the same game tick
    /// with the same priority.
    sub_tick_count: AtomicI64,
    /// Point of interest storage for efficient spatial queries of special blocks.
    pub poi_storage: SyncMutex<PointOfInterestStorage>,
    /// Section-indexed listeners for vanilla game events.
    game_event_listeners: GameEventListenerStorage,
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
        let view_distance = config.view_distance;
        let simulation_distance = config.simulation_distance;
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
        let mut level_data =
            LevelDataManager::new(path, seed, config.difficulty, config.generation_settings)
                .await?;
        if level_data.is_dirty() {
            level_data.save().await?;
        }
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
            let chunk_map = Arc::new(ChunkMap::new_with_storage(
                chunk_runtime,
                weak_self.clone(),
                dimension_type,
                sea_level,
                storage,
                config.generator,
                generation_pool,
            ));
            chunk_map.start_generation_refill_loop();

            Self {
                chunk_map,
                players: PlayerMap::new(),
                player_area_map: PlayerAreaMap::new(),
                key,
                dimension_type,
                level_data: SyncRwLock::new(level_data),
                world_border: SyncMutex::new(world_border),
                view_distance,
                simulation_distance,
                compression,
                is_flat,
                sea_level,
                default_gamemode,
                tick_runs_normally: AtomicBool::new(true),
                entity_manager: WorldEntityManager::new(),
                entity_tracker: EntityTracker::new(),
                navigating_mobs: NavigatingMobTracker::new(),
                weather: SyncMutex::new(weather),
                sub_tick_count: AtomicI64::new(0),
                poi_storage: SyncMutex::new(PointOfInterestStorage::new()),
                game_event_listeners: GameEventListenerStorage::new(),
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

    #[must_use]
    pub(crate) fn world_border_snapshot(&self) -> WorldBorderSnapshot {
        self.world_border.lock().snapshot()
    }

    #[must_use]
    pub(crate) fn initialize_border_packet(&self) -> CInitializeBorder {
        initialize_border_packet(self.world_border_snapshot())
    }

    #[must_use]
    pub(crate) fn world_border_adjusted_respawn_data(
        &self,
        respawn_data: RespawnData,
    ) -> RespawnData {
        let pos = respawn_data.pos();
        let border = self.world_border_snapshot();
        if border.is_within_bounds_with_margin(f64::from(pos.x()), f64::from(pos.z()), 0.0) {
            return respawn_data;
        }

        let center_pos = BlockPos::containing(border.center_x, 0.0, border.center_z);
        let new_pos = self.heightmap_pos(HeightmapType::MotionBlocking, center_pos);
        RespawnData::of(
            respawn_data.dimension().clone(),
            new_pos,
            respawn_data.yaw,
            respawn_data.pitch,
        )
    }

    /// Sets the world border center and broadcasts the vanilla center update packet.
    pub fn set_world_border_center(&self, x: f64, z: f64) -> Result<(), WorldBorderError> {
        let (snapshot, data) = {
            let mut border = self.world_border.lock();
            border.set_center(x, z)?;
            (border.snapshot(), border.to_data())
        };
        self.store_world_border_data_if_changed(data);
        self.broadcast_to_all(CSetBorderCenter {
            new_center_x: snapshot.center_x,
            new_center_z: snapshot.center_z,
        });
        Ok(())
    }

    /// Sets a static world border size and broadcasts the vanilla size update packet.
    pub fn set_world_border_size(&self, size: f64) -> Result<(), WorldBorderError> {
        let (snapshot, data) = {
            let mut border = self.world_border.lock();
            border.set_size(size)?;
            (border.snapshot(), border.to_data())
        };
        self.store_world_border_data_if_changed(data);
        self.broadcast_to_all(CSetBorderSize {
            size: snapshot.new_size,
        });
        Ok(())
    }

    /// Starts a vanilla world border size lerp and broadcasts the lerp update packet.
    pub fn lerp_world_border_size_between(
        &self,
        from: f64,
        to: f64,
        ticks: i64,
    ) -> Result<(), WorldBorderError> {
        let (snapshot, data) = {
            let mut border = self.world_border.lock();
            border.lerp_size_between(from, to, ticks)?;
            (border.snapshot(), border.to_data())
        };
        self.store_world_border_data_if_changed(data);
        self.broadcast_to_all(CSetBorderLerpSize {
            old_size: snapshot.old_size,
            new_size: snapshot.new_size,
            lerp_time: snapshot.lerp_time,
        });
        Ok(())
    }

    /// Sets the client warning time and broadcasts the vanilla warning-delay packet.
    pub fn set_world_border_warning_time(&self, warning_time: i32) {
        let data = {
            let mut border = self.world_border.lock();
            border.set_warning_time(warning_time);
            border.to_data()
        };
        self.store_world_border_data_if_changed(data);
        self.broadcast_to_all(CSetBorderWarningDelay {
            warning_delay: warning_time,
        });
    }

    /// Sets the client warning distance and broadcasts the vanilla warning-distance packet.
    pub fn set_world_border_warning_blocks(&self, warning_blocks: i32) {
        let data = {
            let mut border = self.world_border.lock();
            border.set_warning_blocks(warning_blocks);
            border.to_data()
        };
        self.store_world_border_data_if_changed(data);
        self.broadcast_to_all(CSetBorderWarningDistance { warning_blocks });
    }

    /// Sets world border damage per block outside the safe zone.
    pub fn set_world_border_damage_per_block(
        &self,
        damage_per_block: f64,
    ) -> Result<(), WorldBorderError> {
        let data = {
            let mut border = self.world_border.lock();
            border.set_damage_per_block(damage_per_block)?;
            border.to_data()
        };
        self.store_world_border_data_if_changed(data);
        Ok(())
    }

    /// Sets the safe distance outside the world border before damage starts.
    pub fn set_world_border_safe_zone(&self, safe_zone: f64) -> Result<(), WorldBorderError> {
        let data = {
            let mut border = self.world_border.lock();
            border.set_safe_zone(safe_zone)?;
            border.to_data()
        };
        self.store_world_border_data_if_changed(data);
        Ok(())
    }

    fn tick_world_border(&self) {
        let data = {
            let mut border = self.world_border.lock();
            border.tick();
            border.to_data()
        };
        self.store_world_border_data_if_changed(data);
    }

    fn sync_world_border_to_level_data(&self) {
        let data = self.world_border.lock().to_data();
        self.store_world_border_data_if_changed(data);
    }

    fn store_world_border_data_if_changed(&self, data: WorldBorderData) {
        let mut level_data = self.level_data.write();
        if level_data.data().world_border != data {
            level_data.data_mut().world_border = data;
        }
    }

    fn set_game_time(&self, tick_count: u64) {
        let mut level_data = self.level_data.write();
        level_data.data_mut().game_time = tick_count as i64;
    }

    /// Returns vanilla level game time.
    pub fn game_time(&self) -> i64 {
        self.level_data.read().game_time()
    }

    /// Returns vanilla level difficulty.
    pub fn difficulty(&self) -> Difficulty {
        self.level_data.read().data().difficulty
    }

    /// Returns the total height of the world in blocks.
    pub const fn get_height(&self) -> i32 {
        self.dimension_type.height
    }

    /// Returns the minimum Y coordinate of the world.
    pub const fn get_min_y(&self) -> i32 {
        self.dimension_type.min_y
    }

    /// Returns the maximum Y coordinate of the world.
    pub const fn get_max_y(&self) -> i32 {
        self.get_min_y() + self.get_height() - 1
    }

    /// Returns whether the given Y coordinate is outside the build height.
    pub const fn is_outside_build_height(&self, block_y: i32) -> bool {
        block_y < self.get_min_y() || block_y > self.get_max_y()
    }

    /// Returns whether the block position is within valid horizontal bounds.
    #[expect(clippy::unused_self, reason = "this is an api function")]
    pub const fn is_in_valid_bounds_horizontal(&self, block_pos: BlockPos) -> bool {
        let chunk_x = SectionPos::block_to_section_coord(block_pos.0.x);
        let chunk_z = SectionPos::block_to_section_coord(block_pos.0.z);
        ChunkPos::is_valid(chunk_x, chunk_z)
    }

    /// Returns whether the block position is within valid world bounds.
    pub const fn is_in_valid_bounds(&self, block_pos: BlockPos) -> bool {
        !self.is_outside_build_height(block_pos.0.y)
            && self.is_in_valid_bounds_horizontal(block_pos)
    }

    /// Returns whether the block position is within vanilla spawnable bounds.
    #[must_use]
    pub const fn is_in_spawnable_bounds(block_pos: BlockPos) -> bool {
        !Self::is_outside_spawnable_height(block_pos.0.y)
            && Self::is_in_world_bounds_horizontal(block_pos)
    }

    const fn is_in_world_bounds_horizontal(block_pos: BlockPos) -> bool {
        block_pos.0.x >= -30_000_000
            && block_pos.0.z >= -30_000_000
            && block_pos.0.x < 30_000_000
            && block_pos.0.z < 30_000_000
    }

    const fn is_outside_spawnable_height(y: i32) -> bool {
        y < -20_000_000 || y >= 20_000_000
    }

    /// Returns the maximum build height (one above the highest placeable block).
    /// This is `min_y + height`.
    #[must_use]
    pub const fn max_build_height(&self) -> i32 {
        self.get_min_y() + self.get_height()
    }

    /// Initializes this world's default spawn using vanilla's first-world spawn search.
    pub async fn initialize_spawn_if_needed(self: &Arc<Self>) -> Result<(), String> {
        if self.level_data.read().data().initialized {
            return Ok(());
        }

        if self.dimension_type.key != vanilla_dimension_types::OVERWORLD.key {
            self.level_data.write().data_mut().initialized = true;
            return Ok(());
        }

        log::info!("Selecting global world spawn for {}...", self.key);

        let origin = self
            .chunk_map
            .world_gen_context
            .generator
            .initial_spawn_search_origin();
        let spawn_chunk = ChunkPos::new(
            SectionPos::block_to_section_coord(origin.x()),
            SectionPos::block_to_section_coord(origin.z()),
        );

        let mut spawn_y = self
            .chunk_map
            .world_gen_context
            .generator
            .spawn_height(self.get_min_y(), self.get_height());
        if spawn_y < self.get_min_y() {
            let x = chunk_min_block_x(spawn_chunk) + 8;
            let z = chunk_min_block_z(spawn_chunk) + 8;
            spawn_y = self
                .height_at(HeightmapType::WorldSurface, x, z)
                .unwrap_or(self.get_min_y());
        }

        let mut spawn_pos = BlockPos::new(
            chunk_min_block_x(spawn_chunk) + 8,
            spawn_y,
            chunk_min_block_z(spawn_chunk) + 8,
        );

        spawn_pos = self
            .chunk_map
            .with_full_chunks_in_radius(spawn_chunk, 5, || {
                self.find_spawn_in_loaded_radius(spawn_chunk)
                    .unwrap_or(spawn_pos)
            })
            .await
            .unwrap_or(spawn_pos);

        {
            let mut level_data = self.level_data.write();
            let data = level_data.data_mut();
            data.set_spawn_pos(spawn_pos);
            data.spawn.angle = 0.0;
            data.initialized = true;
        }

        log::info!("World {} spawn initialized at {spawn_pos:?}", self.key);
        Ok(())
    }

    #[expect(
        clippy::similar_names,
        reason = "dx_chunk/dz_chunk mirror vanilla's dXChunk/dZChunk"
    )]
    fn find_spawn_in_loaded_radius(&self, spawn_chunk: ChunkPos) -> Option<BlockPos> {
        let mut x_chunk_offset = 0;
        let mut z_chunk_offset = 0;
        let mut dx_chunk = 0;
        let mut dz_chunk = -1;

        for _ in 0..(11 * 11) {
            if (-5..=5).contains(&x_chunk_offset) && (-5..=5).contains(&z_chunk_offset) {
                let candidate_chunk = ChunkPos::new(
                    spawn_chunk.0.x + x_chunk_offset,
                    spawn_chunk.0.y + z_chunk_offset,
                );
                if let Some(candidate) = self.spawn_pos_in_chunk(candidate_chunk) {
                    return Some(candidate);
                }
            }

            if x_chunk_offset == z_chunk_offset
                || (x_chunk_offset < 0 && x_chunk_offset == -z_chunk_offset)
                || (x_chunk_offset > 0 && x_chunk_offset == 1 - z_chunk_offset)
            {
                let old_dx = dx_chunk;
                dx_chunk = -dz_chunk;
                dz_chunk = old_dx;
            }

            x_chunk_offset += dx_chunk;
            z_chunk_offset += dz_chunk;
        }

        None
    }

    fn spawn_pos_in_chunk(&self, chunk_pos: ChunkPos) -> Option<BlockPos> {
        for x in chunk_min_block_x(chunk_pos)..=chunk_max_block_x(chunk_pos) {
            for z in chunk_min_block_z(chunk_pos)..=chunk_max_block_z(chunk_pos) {
                if let Some(pos) = self.level_respawn_pos(x, z) {
                    return Some(pos);
                }
            }
        }

        None
    }

    fn level_respawn_pos(&self, x: i32, z: i32) -> Option<BlockPos> {
        let top_y = if self.dimension_type.has_ceiling {
            self.chunk_map
                .world_gen_context
                .generator
                .spawn_height(self.get_min_y(), self.get_height())
        } else {
            self.vanilla_chunk_height_at(HeightmapType::MotionBlocking, x, z)?
        };

        if top_y < self.get_min_y() {
            return None;
        }

        let surface = self.vanilla_chunk_height_at(HeightmapType::WorldSurface, x, z)?;
        let ocean_floor = self.vanilla_chunk_height_at(HeightmapType::OceanFloor, x, z)?;
        if surface <= top_y && surface > ocean_floor {
            return None;
        }

        for y in (self.get_min_y()..=top_y + 1).rev() {
            let pos = BlockPos::new(x, y, z);
            let state = self.get_block_state(pos);
            if state.has_fluid() {
                break;
            }

            if is_offset_face_full(state.get_collision_shape_at(pos), Direction::Up) {
                return Some(BlockPos::new(x, y + 1, z));
            }
        }

        None
    }

    fn height_at(&self, heightmap_type: HeightmapType, x: i32, z: i32) -> Option<i32> {
        let chunk_pos = ChunkPos::new(
            SectionPos::block_to_section_coord(x),
            SectionPos::block_to_section_coord(z),
        );
        self.chunk_map.with_full_chunk(chunk_pos, |chunk_access| {
            chunk_access
                .as_full()
                .map(|chunk| chunk.get_height(heightmap_type, (x & 15) as usize, (z & 15) as usize))
        })?
    }

    fn vanilla_chunk_height_at(
        &self,
        heightmap_type: HeightmapType,
        x: i32,
        z: i32,
    ) -> Option<i32> {
        self.height_at(heightmap_type, x, z)
            .map(|first_available| first_available - 1)
    }

    fn heightmap_pos(&self, heightmap_type: HeightmapType, pos: BlockPos) -> BlockPos {
        BlockPos::new(
            pos.x(),
            self.level_height_at(heightmap_type, pos.x(), pos.z()),
            pos.z(),
        )
    }

    fn level_height_at(&self, heightmap_type: HeightmapType, x: i32, z: i32) -> i32 {
        if !Self::is_in_world_bounds_horizontal(BlockPos::new(x, 0, z)) {
            return self.sea_level + 1;
        }

        self.height_at(heightmap_type, x, z)
            .unwrap_or_else(|| self.get_min_y())
    }

    /// Checks if a player may interact with the world at the given position.
    /// Currently only checks if position is within world bounds.
    #[must_use]
    pub const fn may_interact(&self, _player: &Player, pos: BlockPos) -> bool {
        self.is_in_valid_bounds(pos)
    }

    /// Checks if a block's collision shape at the given position is unobstructed by entities.
    ///
    /// This is the Rust equivalent of vanilla's `Level.isUnobstructed(BlockState, BlockPos, CollisionContext)`.
    /// In vanilla, this checks all entities with `blocksBuilding=true` (players, mobs, boats, etc.).
    ///
    /// Returns `true` if the position is clear, `false` if an entity would obstruct placement.
    #[must_use]
    pub fn is_unobstructed(&self, collision_shape: OffsetVoxelShape, pos: BlockPos) -> bool {
        if collision_shape.is_empty() {
            return true;
        }

        for block_aabb in collision_shape.iter() {
            let world_aabb = block_aabb.at_block(pos);
            for entity in self.get_entities_in_aabb(&world_aabb) {
                if entity.blocks_building() && entity.bounding_box().intersects(world_aabb) {
                    return false;
                }
            }
        }

        true
    }

    /// Returns whether the tick rate is running normally.
    ///
    /// When false (frozen/paused), movement validation checks should be skipped.
    /// Matches vanilla's `level.tickRateManager().runsNormally()`.
    #[must_use]
    pub fn tick_runs_normally(&self) -> bool {
        self.tick_runs_normally.load(Ordering::Relaxed)
    }

    /// Sets whether the tick rate is running normally.
    ///
    /// Set to false to freeze/pause the world (e.g., via `/tick freeze` command).
    pub fn set_tick_runs_normally(&self, runs_normally: bool) {
        self.tick_runs_normally
            .store(runs_normally, Ordering::Relaxed);
    }

    /// Gets the value of a game rule.
    /// WARNING: this function acquires a read lock on the level data.
    /// if you already have a write lock on level data, this will DEADLOCK
    #[must_use]
    pub fn get_game_rule(&self, rule: GameRuleRef) -> GameRuleValue {
        let guard = self.level_data.read();
        self.get_game_rule_with_guard(rule, &guard)
    }

    /// Gets the value of a game rule on the `LevelDataManager` guard being passed in.
    #[expect(clippy::unused_self, reason = "this is an api function")]
    #[must_use]
    pub fn get_game_rule_with_guard(
        &self,
        rule: GameRuleRef,
        guard: &LevelDataManager,
    ) -> GameRuleValue {
        guard
            .data()
            .game_rules_values
            .get(rule, &REGISTRY.game_rules)
    }

    /// Sets the value of a game rule.
    /// WARNING: this function acquires a write lock on the level data.
    /// if you already have a read or write lock on level data, this will DEADLOCK
    pub fn set_game_rule(&self, rule: GameRuleRef, value: GameRuleValue) -> bool {
        let mut guard = self.level_data.write();
        self.set_game_rule_with_guard(rule, value, &mut guard)
    }

    /// Sets the value of a game rule on the `LevelDataManager` guard being passed in.
    #[expect(clippy::unused_self, reason = "this is an api function")]
    pub fn set_game_rule_with_guard(
        &self,
        rule: GameRuleRef,
        value: GameRuleValue,
        guard: &mut LevelDataManager,
    ) -> bool {
        guard
            .data_mut()
            .game_rules_values
            .set(rule, value, &REGISTRY.game_rules)
    }

    /// Gets the world seed.
    #[must_use]
    pub fn seed(&self) -> i64 {
        self.level_data.read().data().seed
    }

    /// Gets the obfuscated seed for sending to clients.
    ///
    /// This uses SHA-256 hashing to prevent clients from easily extracting
    /// the actual world seed, matching vanilla's `BiomeManager.obfuscateSeed()`.
    #[must_use]
    #[expect(
        clippy::missing_panics_doc,
        reason = "panic is unreachable: SHA-256 always produces 32 bytes"
    )]
    pub fn obfuscated_seed(&self) -> i64 {
        let seed = self.seed();
        let mut hasher = Sha256::new();
        hasher.update(seed.to_be_bytes());
        let result = hasher.finalize();
        // SHA-256 always produces 32 bytes, so taking 8 bytes always succeeds
        let bytes: [u8; 8] = result[0..8].try_into().expect("SHA-256 produces 32 bytes");
        i64::from_be_bytes(bytes)
    }

    /// Gets the block state at the given position.
    ///
    /// Returns the default block state (void air) if the position is out of bounds or the chunk is not loaded.
    #[must_use]
    pub fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        if !self.is_in_valid_bounds(pos) {
            return REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR);
        }

        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map
            .with_full_chunk(chunk_pos, |chunk| chunk.get_block_state(pos))
            .unwrap_or_else(|| REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR))
    }

    pub(crate) fn queue_light_change_after_block_set(
        &self,
        pos: BlockPos,
        old_state: BlockStateId,
        new_state: BlockStateId,
        empty_section_change: Option<LightSectionEmptinessChange>,
    ) {
        let light_properties_changed = has_different_light_properties(old_state, new_state);
        if !light_properties_changed && empty_section_change.is_none() {
            return;
        }

        self.chunk_map
            .queue_light_change(pos, light_properties_changed, empty_section_change);
    }

    fn light_value_at(&self, layer: LightLayer, pos: BlockPos) -> u8 {
        if layer == LightLayer::Sky && !self.dimension_type.has_skylight {
            return 0;
        }
        if !self.is_in_valid_bounds_horizontal(pos) {
            return self.default_light_value(layer);
        }

        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map
            .with_chunk_at_status(chunk_pos, ChunkStatus::Light, |chunk| {
                let light = chunk.light();
                light.get_light_value(layer, pos)
            })
            .unwrap_or_else(|| self.default_light_value(layer))
    }

    const fn default_light_value(&self, layer: LightLayer) -> u8 {
        match layer {
            LightLayer::Sky if self.dimension_type.has_skylight => MAX_LIGHT_LEVEL,
            LightLayer::Sky | LightLayer::Block => 0,
        }
    }

    /// Returns whether every block state in the vanilla AABB block range is air.
    ///
    /// Matches `BlockGetter.getBlockStates(AABB)` using
    /// `BlockPos.betweenClosedStream(AABB)`: both min and max coordinates are
    /// floored before iterating the inclusive block range.
    #[must_use]
    pub fn block_states_in_aabb_are_air(&self, aabb: WorldAabb) -> bool {
        let min_x = aabb.min_x().floor() as i32;
        let min_y = aabb.min_y().floor() as i32;
        let min_z = aabb.min_z().floor() as i32;
        let max_x = aabb.max_x().floor() as i32;
        let max_y = aabb.max_y().floor() as i32;
        let max_z = aabb.max_z().floor() as i32;

        for y in min_y..=max_y {
            for z in min_z..=max_z {
                for x in min_x..=max_x {
                    if !self.get_block_state(BlockPos::new(x, y, z)).is_air() {
                        return false;
                    }
                }
            }
        }

        true
    }

    /// Gets a block state for generation postprocessing.
    ///
    /// Vanilla delays `LevelChunk.postProcessGeneration` until neighboring
    /// chunks are full because that hook runs during the ticking-chunk
    /// transition. Steel runs it as the center chunk reaches full. At that
    /// point the chunk pyramid guarantees the 3x3 neighbors have reached
    /// `Light`, which means they have completed `Features`, the last
    /// block-mutating generation stage. Postprocessing only needs block
    /// states, so reading light-stage proto chunks here is intentional.
    #[must_use]
    pub(crate) fn get_postprocessing_block_state(&self, pos: BlockPos) -> BlockStateId {
        if !self.is_in_valid_bounds(pos) {
            return REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR);
        }

        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map
            .with_chunk_at_status(chunk_pos, ChunkStatus::Features, |chunk| {
                chunk.get_block_state(pos)
            })
            .unwrap_or_else(|| REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR))
    }

    /// Sets a block at the given position.
    ///
    /// Returns `true` if the block was successfully set, `false` otherwise.
    /// Uses the default update limit of 512 (matching vanilla).
    pub fn set_block(
        self: &Arc<Self>,
        pos: BlockPos,
        block_state: BlockStateId,
        flags: UpdateFlags,
    ) -> bool {
        self.set_block_with_limit(pos, block_state, flags, 512)
    }

    /// Sets a block at the given position with a custom update limit.
    ///
    /// The update limit prevents infinite recursion when shape updates trigger
    /// further block changes. Each recursive call decrements the limit.
    ///
    /// Returns `true` if the block was successfully set, `false` otherwise.
    pub fn set_block_with_limit(
        self: &Arc<Self>,
        pos: BlockPos,
        block_state: BlockStateId,
        flags: UpdateFlags,
        update_limit: i32,
    ) -> bool {
        if update_limit <= 0 {
            return false;
        }

        if !self.is_in_valid_bounds(pos) {
            return false;
        }

        let chunk_pos = Self::chunk_pos_for_block(pos);
        let Some(old_state) = self
            .chunk_map
            .with_full_chunk(chunk_pos, |chunk| {
                chunk.set_block_state(pos, block_state, flags)
            })
            .flatten()
        else {
            return false;
        };

        // Record the block change for broadcasting to clients
        self.chunk_map.block_changed(pos);
        self.update_navigating_mobs_after_block_collision_change(pos, old_state, block_state);

        // Neighbor updates (when UPDATE_NEIGHBORS is set)
        if flags.contains(UpdateFlags::UPDATE_NEIGHBORS) {
            self.update_neighbors_at(pos, old_state.get_block());
            // TODO: if block has analog output signal, update comparator neighbors
            // via updateNeighborForOutputSignal
        }

        // Shape updates (unless UPDATE_KNOWN_SHAPE is set)
        if !flags.contains(UpdateFlags::UPDATE_KNOWN_SHAPE) && update_limit > 0 {
            // Clear UPDATE_NEIGHBORS and UPDATE_SUPPRESS_DROPS for propagation
            let neighbor_flags =
                flags & !(UpdateFlags::UPDATE_NEIGHBORS | UpdateFlags::UPDATE_SUPPRESS_DROPS);

            // Notify all 6 neighbors about our shape change
            for direction in Direction::UPDATE_SHAPE_ORDER {
                let neighbor_pos = pos.relative(direction);

                // Tell the neighbor that we (at pos) changed
                self.neighbor_shape_changed(
                    direction.opposite(), // Direction from us to neighbor
                    neighbor_pos,         // Neighbor's position
                    pos,                  // Our position (the one that changed)
                    block_state,          // Our new state
                    neighbor_flags,
                    update_limit - 1,
                );
            }
        }
        true
    }

    fn update_navigating_mobs_after_block_collision_change(
        self: &Arc<Self>,
        pos: BlockPos,
        old_state: BlockStateId,
        new_state: BlockStateId,
    ) {
        let collision_shape_changed = self.block_collision_shape_changed(pos, old_state, new_state);
        let game_time = self.game_time();
        for entity_id in self.navigating_mob_ids() {
            let Some(entity) = self.entity_manager.get_by_id(entity_id) else {
                self.untrack_navigating_mob(entity_id);
                continue;
            };
            let Some(pathfinder) = entity.as_pathfinder_mob() else {
                self.untrack_navigating_mob(entity_id);
                continue;
            };
            {
                let mut navigation = pathfinder.mob_base().navigation().lock();
                navigation.invalidate_path_type(pos);
            }
            if !collision_shape_changed {
                continue;
            }
            if !pathfinder.is_path_finding() {
                continue;
            }

            let should_recompute = {
                let navigation = pathfinder.mob_base().navigation().lock();
                navigation.should_recompute_path(pos, pathfinder.position())
            };
            if !should_recompute {
                continue;
            }

            let request = {
                let mut navigation = pathfinder.mob_base().navigation().lock();
                navigation.request_recompute_path(game_time, pathfinder.can_update_path())
            };
            if let Some(request) = request {
                pathfinder.recompute_path(request);
            }
        }
    }

    fn navigating_mob_ids(&self) -> Vec<i32> {
        self.navigating_mobs.ids()
    }

    fn block_collision_shape_changed(
        &self,
        pos: BlockPos,
        old_state: BlockStateId,
        new_state: BlockStateId,
    ) -> bool {
        let old_shape = self.block_collision_shape(pos, old_state);
        let new_shape = self.block_collision_shape(pos, new_state);
        join_is_not_empty(old_shape, new_shape, BooleanOp::NotSame)
    }

    fn block_collision_shape(&self, pos: BlockPos, state: BlockStateId) -> VoxelShape {
        BLOCK_BEHAVIORS
            .get_behavior(state.get_block())
            .get_collision_shape(state, self, pos, BlockCollisionContext::empty())
    }

    /// Order in which neighbors are updated (matches vanilla's `NeighborUpdater.UPDATE_ORDER`).
    const NEIGHBOR_UPDATE_ORDER: [Direction; 6] = [
        Direction::West,
        Direction::East,
        Direction::Down,
        Direction::Up,
        Direction::North,
        Direction::South,
    ];

    /// Updates all neighbors of the given position about a block change.
    ///
    /// This is the Rust equivalent of vanilla's `Level.updateNeighborsAt()`.
    pub fn update_neighbors_at(self: &Arc<Self>, pos: BlockPos, source_block: BlockRef) {
        for direction in Self::NEIGHBOR_UPDATE_ORDER {
            let neighbor_pos = pos.relative(direction);
            self.neighbor_changed(neighbor_pos, source_block, false);
        }
    }

    /// Called when a neighbor's shape changes, to update this block's state.
    ///
    /// This is the Rust equivalent of vanilla's `NeighborUpdater.executeShapeUpdate()`.
    fn neighbor_shape_changed(
        self: &Arc<Self>,
        direction: Direction,
        pos: BlockPos,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
        flags: UpdateFlags,
        update_limit: i32,
    ) {
        if !self.is_in_valid_bounds(pos) {
            return;
        }

        let current_state = self.get_block_state(pos);

        if flags.contains(UpdateFlags::UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE)
            && current_state.get_block() == &vanilla_blocks::REDSTONE_WIRE
        {
            return;
        }

        let block_behaviors = &*BLOCK_BEHAVIORS;
        let behavior = block_behaviors.get_behavior(current_state.get_block());
        let new_state = behavior.update_shape(
            current_state,
            self,
            pos,
            direction,
            neighbor_pos,
            neighbor_state,
        );

        self.update_or_destroy(current_state, new_state, pos, flags, update_limit);

        // Vanilla parity: `SimpleWaterloggedBlock.updateShape` / `Level.neighborShapeChanged` —
        // always reschedule the fluid tick when a block with fluid has a neighbor shape change,
        // regardless of whether the block state itself changed. This ensures waterlogged blocks
        // (fences, slabs, stairs…) propagate their fluid when adjacent blocks are removed.
        let fluid_state = new_state.get_fluid_state();
        if !fluid_state.is_empty() {
            let delay = FLUID_BEHAVIORS
                .get_behavior(fluid_state.fluid_id)
                .tick_delay(self);
            self.schedule_fluid_tick_default(pos, fluid_state.fluid_id, delay);
        }
    }

    fn update_or_destroy(
        self: &Arc<World>,
        old_state: BlockStateId,
        new_state: BlockStateId,
        pos: BlockPos,
        flags: UpdateFlags,
        recursion_left: i32,
    ) {
        if new_state == old_state {
            return;
        }

        if new_state.is_air() {
            self.destroy_block(pos, !flags.contains(UpdateFlags::UPDATE_SUPPRESS_DROPS));
        } else {
            self.set_block_with_limit(pos, new_state, flags, recursion_left);
        }
    }

    /// Notifies a block that one of its neighbors changed.
    ///
    /// This is the Rust equivalent of vanilla's `Level.neighborChanged()`.
    pub(crate) fn neighbor_changed(
        self: &Arc<Self>,
        pos: BlockPos,
        source_block: BlockRef,
        moved_by_piston: bool,
    ) {
        if !self.is_in_valid_bounds(pos) {
            return;
        }

        let state = self.get_block_state(pos);
        let block_behaviors = &*BLOCK_BEHAVIORS;
        let behavior = block_behaviors.get_behavior(state.get_block());
        behavior.handle_neighbor_changed(state, self, pos, source_block, moved_by_piston);
    }

    const fn chunk_pos_for_block(pos: BlockPos) -> ChunkPos {
        ChunkPos::new(
            SectionPos::block_to_section_coord(pos.0.x),
            SectionPos::block_to_section_coord(pos.0.z),
        )
    }

    /// Gets a block entity at the given position.
    ///
    /// Returns `None` if the chunk is not loaded or there is no block entity at the position.
    #[must_use]
    pub fn get_block_entity(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map
            .with_full_chunk(chunk_pos, |chunk| {
                chunk.as_full().and_then(|lc| lc.get_block_entity(pos))
            })
            .flatten()
    }

    /// Called when a block entity's data changes.
    ///
    /// Marks the containing chunk as unsaved so it will be persisted to disk.
    pub fn block_entity_changed(&self, pos: BlockPos) {
        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map.packet_content_changed(chunk_pos);
        self.mark_chunk_dirty(chunk_pos);
    }

    /// Marks a chunk as dirty (unsaved) so it will be persisted to disk.
    ///
    /// Called when entities move, are added/removed, or when block entities change.
    pub fn mark_chunk_dirty(&self, chunk_pos: ChunkPos) {
        self.chunk_map
            .with_chunk_at_status(chunk_pos, ChunkStatus::Empty, ChunkAccess::mark_dirty);
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
        self.set_game_time(tick_count);
        self.set_tick_runs_normally(runs_normally);
        if runs_normally {
            self.tick_world_border();
            self.tick_weather();
            self.tick_time();
        }

        let random_tick_speed = self.get_game_rule(&RANDOM_TICK_SPEED).as_int().unwrap_or(3) as u32;

        let mut chunk_map_timings =
            self.chunk_map
                .tick_game(self, tick_count, random_tick_speed, runs_normally);

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

        self.chunk_map
            .tick_block_entities(&mut chunk_map_timings, runs_normally);

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

        if tick_count.is_multiple_of(SEND_PLAYER_INFO_INTERVAL) {
            let _span = tracing::trace_span!("broadcast_latency").entered();
            self.broadcast_player_latency_updates();
        }

        WorldGameTickTimings {
            elapsed: world_start.elapsed(),
            chunk_map: chunk_map_timings,
            entity_tick,
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "splitting would hurt readability of the weather state machine"
    )]
    fn tick_weather(&self) {
        if !self.can_have_weather() {
            return;
        }

        let mut weather = self.weather.lock();
        let raining_before = self.is_raining_with_guard(&weather);

        // Advance the weather state machine (only if gamerule allows)
        {
            let mut level_data = self.level_data.write();

            if self
                .get_game_rule_with_guard(&ADVANCE_WEATHER, &level_data)
                .as_bool()
                .expect("gamerule `ADVANCE_WEATHER` should always be a boolean.")
            {
                let clear_weather_time = level_data.clear_weather_time();
                if clear_weather_time > 0 {
                    level_data.set_clear_weather_time(clear_weather_time - 1);
                    if level_data.is_thundering() {
                        level_data.set_thunder_time(0);
                        level_data.set_thundering(false);
                    } else {
                        level_data.set_thunder_time(1);
                    }
                    if level_data.is_raining() {
                        level_data.set_rain_time(0);
                        level_data.set_raining(false);
                    } else {
                        level_data.set_rain_time(1);
                    }
                } else {
                    let thundering_time = level_data.thunder_time();
                    if thundering_time > 0 {
                        level_data.set_thunder_time(thundering_time - 1);
                        if level_data.thunder_time() == 0 {
                            let thundering = level_data.is_thundering();
                            level_data.set_thundering(!thundering);
                        }
                    } else if level_data.is_thundering() {
                        level_data.set_thunder_time(rand::random_range(3_600..=15_600));
                    } else {
                        level_data.set_thunder_time(rand::random_range(12_000..=180_000));
                    }

                    let rain_time = level_data.rain_time();
                    if rain_time > 0 {
                        level_data.set_rain_time(rain_time - 1);
                        if level_data.rain_time() == 0 {
                            let raining = level_data.is_raining();
                            level_data.set_raining(!raining);
                        }
                    } else if level_data.is_raining() {
                        level_data.set_rain_time(rand::random_range(12_000..=24_000));
                    } else {
                        level_data.set_rain_time(rand::random_range(12_000..=180_000));
                    }
                }
            }
        }

        // Interpolate visual levels (always runs, even when ADVANCE_WEATHER is off)
        let is_thundering = self.level_data.read().is_thundering();
        let is_raining = self.level_data.read().is_raining();

        weather.previous_thunder_level = weather.thunder_level;
        if is_thundering {
            weather.thunder_level += 0.01;
        } else {
            weather.thunder_level -= 0.01;
        }
        weather.thunder_level = weather.thunder_level.clamp(0.0, 1.0);

        weather.previous_rain_level = weather.rain_level;
        if is_raining {
            weather.rain_level += 0.01;
        } else {
            weather.rain_level -= 0.01;
        }
        weather.rain_level = weather.rain_level.clamp(0.0, 1.0);

        // Broadcast weather changes to clients
        let raining_now = self.is_raining_with_guard(&weather);
        if raining_before == raining_now {
            #[expect(
                clippy::float_cmp,
                reason = "comparing against the exact previously-assigned value to detect any change"
            )]
            if weather.previous_rain_level != weather.rain_level {
                self.broadcast_to_all(CGameEvent {
                    event: GameEventType::RainLevelChange,
                    data: weather.rain_level,
                });
            }

            #[expect(
                clippy::float_cmp,
                reason = "comparing against the exact previously-assigned value to detect any change"
            )]
            if weather.previous_thunder_level != weather.thunder_level {
                self.broadcast_to_all(CGameEvent {
                    event: GameEventType::ThunderLevelChange,
                    data: weather.thunder_level,
                });
            }
        } else {
            if raining_before {
                self.broadcast_to_all(CGameEvent {
                    event: GameEventType::StopRaining,
                    data: 0.0,
                });
            } else {
                self.broadcast_to_all(CGameEvent {
                    event: GameEventType::StartRaining,
                    data: 0.0,
                });
            }

            self.broadcast_to_all(CGameEvent {
                event: GameEventType::RainLevelChange,
                data: weather.rain_level,
            });

            self.broadcast_to_all(CGameEvent {
                event: GameEventType::ThunderLevelChange,
                data: weather.thunder_level,
            });
        }
    }

    /// Checks whether the rain level is high enough to be considered raining.
    /// Used for both visual rendering and gameplay logic (crop growth, fire, mob behavior).
    ///
    /// WARNING: this function acquires a lock on the `weather` field.
    /// if you already have a lock on the `weather` field, this will DEADLOCK.
    pub fn is_raining(&self) -> bool {
        let guard = self.weather.lock();
        self.is_raining_with_guard(&guard)
    }

    /// Checks whether rain reaches the given block position.
    ///
    /// Mirrors vanilla `Level.isRainingAt`: global rain state, sky exposure,
    /// motion-blocking height, and biome precipitation must all allow rain.
    pub fn is_raining_at(&self, pos: BlockPos) -> bool {
        if !self.is_raining() || !self.can_see_sky_for_precipitation(pos) {
            return false;
        }

        self.biome_at(pos).is_some_and(|biome| {
            biome.has_precipitation && self.biome_temperature(biome, pos) >= 0.15
        })
    }

    /// Checks whether the rain level is sufficient to render rain clientside using the provided guard.
    pub fn is_raining_with_guard(&self, guard: &Weather) -> bool {
        guard.rain_level > 0.2 && self.can_have_weather()
    }

    /// Checks whether the thunder level and rain level are high enough to be considered thundering.
    /// Used for lightning spawning and gameplay logic.
    ///
    /// WARNING: this function acquires a lock on the `weather` field.
    /// if you already have a lock on the `weather` field, this will DEADLOCK.
    pub fn is_thundering(&self) -> bool {
        let guard = self.weather.lock();
        self.is_thundering_with_guard(&guard)
    }

    /// Checks whether the thunder level and rain level are sufficient to spawn thunderbolts using the provided guard.
    pub fn is_thundering_with_guard(&self, guard: &Weather) -> bool {
        guard.rain_level * guard.thunder_level > 0.9 && self.can_have_weather()
    }

    /// Returns the current vanilla `SKY_LIGHT_LEVEL` environment attribute.
    pub fn sky_light_level(&self) -> f32 {
        let day_time = self.level_data.read().day_time();
        let (rain_level, thunder_level) = if self.can_have_weather() {
            let weather = self.weather.lock();
            (weather.rain_level, weather.thunder_level)
        } else {
            (0.0, 0.0)
        };

        environment::sky_light_level(
            self.dimension_type,
            day_time,
            rain_level,
            thunder_level,
            self.can_have_weather(),
        )
    }

    /// Returns vanilla `Level.skyDarken`.
    pub fn sky_darkening(&self) -> u8 {
        environment::sky_darkening(self.sky_light_level())
    }

    /// Returns vanilla `Level.isBrightOutside`.
    pub fn is_bright_outside(&self) -> bool {
        self.dimension_type.fixed_time.is_none() && self.sky_darkening() < 4
    }

    /// Returns vanilla `Level.isDarkOutside`.
    pub fn is_dark_outside(&self) -> bool {
        self.dimension_type.fixed_time.is_none() && !self.is_bright_outside()
    }

    /// Checks whether the world can have weather.
    pub fn can_have_weather(&self) -> bool {
        self.dimension_type.has_skylight
            && !self.dimension_type.has_ceiling
            && self.dimension_type.key != vanilla_dimension_types::THE_END.key
    }

    /// Returns whether the position has unobstructed sky exposure.
    ///
    /// Live worlds use the motion-blocking heightmap until Steel has a full
    /// live sky-light engine.
    pub fn can_see_sky(&self, pos: BlockPos) -> bool {
        if !self.dimension_type.has_skylight {
            return false;
        }
        self.height_at(HeightmapType::MotionBlocking, pos.x(), pos.z())
            .is_some_and(|height| height <= pos.y())
    }

    fn can_see_sky_for_precipitation(&self, pos: BlockPos) -> bool {
        self.can_see_sky(pos)
    }

    pub(crate) fn biome_at(&self, pos: BlockPos) -> Option<BiomeRef> {
        let biome_zoom_seed = obfuscate_biome_seed(self.seed());
        let mut missing_chunk = false;
        let biome_id = fuzzed_biome_at_block(biome_zoom_seed, pos, |quart| {
            self.noise_biome_id(quart.x, quart.y, quart.z)
                .unwrap_or_else(|| {
                    missing_chunk = true;
                    0
                })
        });

        if missing_chunk {
            return None;
        }

        REGISTRY.biomes.by_id(usize::from(biome_id))
    }

    fn noise_biome_id(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> Option<u16> {
        let chunk_pos = ChunkPos::new(quart_x >> 2, quart_z >> 2);
        let local_quart_x = (quart_x & 3) as usize;
        let local_quart_z = (quart_z & 3) as usize;

        self.chunk_map.with_full_chunk(chunk_pos, |chunk| {
            let sections = chunk.sections();
            let (section_index, local_quart_y) =
                Self::biome_quart_y_indices(chunk.min_y(), sections.sections.len(), quart_y)?;
            let section = sections.sections.get(section_index)?;
            Some(
                section
                    .read()
                    .biomes
                    .get(local_quart_x, local_quart_y, local_quart_z),
            )
        })?
    }

    fn biome_quart_y_indices(
        min_y: i32,
        section_count: usize,
        quart_y: i32,
    ) -> Option<(usize, usize)> {
        let total_quart_y = section_count.checked_mul(4)?;
        if total_quart_y == 0 {
            return None;
        }

        let relative_quart_y = i64::from(quart_y) - i64::from(min_y >> 2);
        let max_relative_quart_y = total_quart_y - 1;
        let clamped_relative_quart_y = if relative_quart_y <= 0 {
            0
        } else {
            usize::try_from(relative_quart_y).map_or(max_relative_quart_y, |relative| {
                relative.min(max_relative_quart_y)
            })
        };

        Some((clamped_relative_quart_y / 4, clamped_relative_quart_y & 3))
    }

    fn biome_temperature(&self, biome: BiomeRef, pos: BlockPos) -> f32 {
        let modified_temp = match biome.temperature_modifier {
            TemperatureModifier::None => biome.temperature,
            TemperatureModifier::Frozen => {
                let large = FROZEN_BIOME_TEMPERATURE_NOISE
                    .get_value(f64::from(pos.x()) * 0.05, f64::from(pos.z()) * 0.05)
                    * 7.0;
                let edge =
                    BIOME_INFO_NOISE.get_value(f64::from(pos.x()) * 0.2, f64::from(pos.z()) * 0.2);
                if large + edge < 0.3 {
                    let small = BIOME_INFO_NOISE
                        .get_value(f64::from(pos.x()) * 0.09, f64::from(pos.z()) * 0.09);
                    if small < 0.8 {
                        return 0.2;
                    }
                }
                biome.temperature
            }
        };

        let snow_level = self.sea_level + 17;
        if pos.y() <= snow_level {
            return modified_temp;
        }

        let noise = BIOME_TEMPERATURE_NOISE
            .get_value(f64::from(pos.x()) / 8.0, f64::from(pos.z()) / 8.0)
            as f32
            * 8.0;
        modified_temp - (noise + pos.y() as f32 - snow_level as f32) * 0.05 / 40.0
    }

    /// Schedules a block tick at the given position.
    ///
    /// The tick will fire after `delay` game ticks with the given priority.
    /// Only one tick per `(pos, block)` pair can be active at a time — duplicates
    /// are silently ignored.
    pub fn schedule_block_tick(
        &self,
        pos: BlockPos,
        block: BlockRef,
        delay: i32,
        priority: tick_scheduler::TickPriority,
    ) {
        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map.with_full_chunk(chunk_pos, |chunk_access| {
            if let Some(chunk) = chunk_access.as_full() {
                let order = self.sub_tick_count.fetch_add(1, Ordering::Relaxed);
                let tick = tick_scheduler::BlockTick {
                    tick_type: block,
                    pos,
                    delay,
                    priority,
                    sub_tick_order: order,
                };
                chunk.block_ticks.lock().schedule(tick);
            }
        });
    }

    /// Schedules a block tick with `Normal` priority.
    pub fn schedule_block_tick_default(&self, pos: BlockPos, block: BlockRef, delay: i32) {
        self.schedule_block_tick(pos, block, delay, tick_scheduler::TickPriority::Normal);
    }

    /// Schedules a fluid tick at the given position.
    ///
    /// The tick will fire after `delay` game ticks with the given priority.
    /// Only one tick per `(pos, fluid)` pair can be active at a time.
    pub fn schedule_fluid_tick(
        &self,
        pos: BlockPos,
        fluid: FluidRef,
        delay: i32,
        priority: tick_scheduler::TickPriority,
    ) {
        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map.with_full_chunk(chunk_pos, |chunk_access| {
            if let Some(chunk) = chunk_access.as_full() {
                let order = self.sub_tick_count.fetch_add(1, Ordering::Relaxed);
                let tick = tick_scheduler::FluidTick {
                    tick_type: fluid,
                    pos,
                    delay,
                    priority,
                    sub_tick_order: order,
                };
                chunk.fluid_ticks.lock().schedule(tick);
            }
        });
    }

    /// Schedules a fluid tick with `Normal` priority.
    pub fn schedule_fluid_tick_default(&self, pos: BlockPos, fluid: FluidRef, delay: i32) {
        self.schedule_fluid_tick(pos, fluid, delay, tick_scheduler::TickPriority::Normal);
    }

    /// Returns `true` if a block tick is already scheduled for the given `(pos, block)`.
    pub fn has_scheduled_block_tick(&self, pos: BlockPos, block: BlockRef) -> bool {
        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map
            .with_full_chunk(chunk_pos, |chunk_access| {
                chunk_access
                    .as_full()
                    .is_some_and(|chunk| chunk.block_ticks.lock().has_tick(pos, block))
            })
            .unwrap_or(false)
    }

    /// Returns `true` if a fluid tick is already scheduled for the given `(pos, fluid)`.
    pub fn has_scheduled_fluid_tick(&self, pos: BlockPos, fluid: FluidRef) -> bool {
        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map
            .with_full_chunk(chunk_pos, |chunk_access| {
                chunk_access
                    .as_full()
                    .is_some_and(|chunk| chunk.fluid_ticks.lock().has_tick(pos, fluid))
            })
            .unwrap_or(false)
    }

    /// Advances the gametime and the daytime (if `ADVANCE_TIME` gamerule is true) by one tick, and
    /// then sends an update to all clients in this world every 20th tick.
    fn tick_time(&self) {
        let advance_time = self
            .get_game_rule(&ADVANCE_TIME)
            .as_bool()
            .expect("gamerule advance_time should always be a bool.");

        let (game_time, day_time) = {
            let mut lock = self.level_data.write();
            let updated_game_time = lock.game_time() + 1;
            lock.set_game_time(updated_game_time);
            let current_day_time = lock.day_time();

            if advance_time {
                let updated_day_time = (current_day_time + 1) % 24000;
                lock.set_day_time(updated_day_time);
                (updated_game_time, updated_day_time)
            } else {
                (updated_game_time, current_day_time)
            }
        };

        if game_time % 20 == 0 {
            let rate = if advance_time { 1.0 } else { 0.0 };
            self.broadcast_to_all(CSetTime::new(game_time, day_time, 0.0, rate));
        }
    }

    /// Broadcasts latency updates for all players to all players.
    /// This is called every `SEND_PLAYER_INFO_INTERVAL` ticks to update the ping display.
    fn broadcast_player_latency_updates(&self) {
        // Collect all player latencies
        let mut latency_entries = Vec::new();
        self.players.iter_players(|uuid, player| {
            latency_entries.push((*uuid, player.connection.latency()));
            true
        });

        // Only broadcast if there are players
        if !latency_entries.is_empty() {
            let packet = CPlayerInfoUpdate::update_latency(latency_entries);
            self.broadcast_to_all(packet);
        }
    }

    /// Broadcasts a signed chat message to all players in the world.
    ///
    /// # Panics
    /// Panics if `message_signature` is `None` after checking `is_some()` (should never happen).
    pub fn broadcast_chat(
        &self,
        mut packet: CPlayerChat,
        _sender: Arc<Player>,
        sender_last_seen: LastSeen,
        message_signature: Option<&[u8; 256]>,
    ) {
        log::debug!(
            "broadcast_chat: sender_last_seen has {} signatures, message_signature present: {}",
            sender_last_seen.len(),
            message_signature.is_some()
        );

        self.players.iter_players(|_, recipient| {
            let messages_received = recipient.get_and_increment_messages_received();
            packet.global_index = messages_received;

            log::debug!(
                "Broadcasting to player {} (UUID: {}), global_index={}",
                recipient.gameprofile.name,
                recipient.gameprofile.id,
                messages_received
            );

            // IMPORTANT: Index previous messages BEFORE updating the cache
            // This matches vanilla's order: pack() then push()
            let previous_messages = {
                let chat = recipient.chat.lock();
                chat.signature_cache
                    .index_previous_messages(&sender_last_seen)
            };

            log::debug!(
                "  Indexed {} previous messages for recipient",
                previous_messages.len()
            );

            packet.previous_messages.clone_from(&previous_messages);

            // Send the packet
            recipient.send_packet(packet.clone());

            // AFTER sending, update the recipient's cache using vanilla's push algorithm
            // This adds all lastSeen signatures + current signature to the cache
            {
                let mut chat = recipient.chat.lock();
                if let Some(signature) = message_signature {
                    chat.signature_cache
                        .push(&sender_last_seen, Some(signature));

                    log::debug!("  Added signature to recipient's cache and pending list");

                    // Add to pending messages for acknowledgment tracking
                    chat.message_validator
                        .add_pending(Some(Box::new(*signature) as Box<[u8]>));
                } else {
                    // Even unsigned messages update the pending tracker
                    chat.message_validator.add_pending(None);
                    log::debug!("  Added unsigned message to pending list");
                }
            }

            true
        });
    }

    /// Broadcasts a system chat message to all players.
    pub fn broadcast_system_chat(&self, packet: CSystemChat) {
        self.broadcast_to_all(packet);
    }

    /// Broadcasts a packet to all players in the world.
    pub fn broadcast_to_all<P: ClientPacket>(&self, packet: P) {
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            return;
        };
        self.broadcast_to_all_encoded(encoded);
    }

    /// Broadcasts a packet to all players in the world except one (identified by entity ID).
    pub fn broadcast_to_all_except<P: ClientPacket>(&self, packet: P, exclude: i32) {
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            return;
        };
        self.broadcast_to_all_encoded_except(encoded, exclude);
    }

    /// Broadcasts a packet to all players in the world.
    ///
    /// This method handles encoding the packets produced from the function passed.
    pub fn broadcast_to_all_with<P: ClientPacket, F: Fn(&Player) -> P>(&self, packet: F) {
        self.players.iter_players(|_, player| {
            let Ok(encoded) = EncodedPacket::from_bare(
                packet(player),
                self.compression,
                ConnectionProtocol::Play,
            ) else {
                return false;
            };
            player.connection.send_encoded(encoded);
            true
        });
    }

    /// Broadcasts an already-encoded packet to all players in the world.
    pub fn broadcast_to_all_encoded(&self, packet: EncodedPacket) {
        self.players.iter_players(|_, player| {
            player.connection.send_encoded(packet.clone());
            true
        });
    }

    /// Broadcasts an already-encoded packet to all players except one.
    pub fn broadcast_to_all_encoded_except(&self, packet: EncodedPacket, exclude: i32) {
        self.players.iter_players(|_, player| {
            if player.id() != exclude {
                player.connection.send_encoded(packet.clone());
            }
            true
        });
    }

    /// Broadcasts an unsigned player chat message to all players.
    pub fn broadcast_unsigned_chat(&self, mut packet: CPlayerChat) {
        self.players.iter_players(|_, recipient| {
            let messages_received = recipient.get_and_increment_messages_received();
            packet.global_index = messages_received;

            recipient.send_packet(packet.clone());
            true
        });
    }

    /// Broadcasts a packet to all players tracking the given chunk.
    ///
    /// This method handles encoding the packet internally, avoiding boilerplate at call sites.
    /// If encoding fails, the broadcast is silently skipped.
    pub fn broadcast_to_nearby<P: ClientPacket>(
        &self,
        chunk: ChunkPos,
        packet: P,
        exclude: Option<i32>,
    ) {
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            return;
        };
        self.broadcast_to_nearby_encoded(chunk, encoded, exclude);
    }

    /// Broadcasts an already-encoded packet to all players tracking the given chunk.
    ///
    /// Use this when you have a pre-encoded packet to avoid re-encoding.
    pub fn broadcast_to_nearby_encoded(
        &self,
        chunk: ChunkPos,
        packet: EncodedPacket,
        exclude: Option<i32>,
    ) {
        let tracking_players = self.get_packet_tracking_players(chunk);
        for entity_id in tracking_players {
            if Some(entity_id) == exclude {
                continue;
            }
            if let Some(player) = self.players.get_by_entity_id(entity_id) {
                player.connection.send_encoded(packet.clone());
            }
        }
    }

    /// Returns players whose view includes the chunk and whose client has the base chunk packet.
    pub fn get_packet_tracking_players(&self, chunk: ChunkPos) -> Vec<i32> {
        self.player_area_map
            .get_tracking_players(chunk)
            .into_iter()
            .filter(|entity_id| {
                self.players
                    .get_by_entity_id(*entity_id)
                    .is_some_and(|player| player.chunk_sender.lock().is_chunk_sent(chunk))
            })
            .collect()
    }

    /// Returns players on the tracked border of a chunk whose client has its base chunk packet.
    pub fn get_light_packet_tracking_players(&self, chunk: ChunkPos) -> Vec<i32> {
        self.player_area_map
            .get_tracking_players(chunk)
            .into_iter()
            .filter(|entity_id| {
                let Some(player) = self.players.get_by_entity_id(*entity_id) else {
                    return false;
                };
                let Some(view) = *player.last_tracking_view.lock() else {
                    return false;
                };
                let chunk_sender = player.chunk_sender.lock();
                let is_chunk_sent = |pos| chunk_sender.is_chunk_sent(pos);
                Self::chunk_is_on_packet_tracked_border(view, chunk, &is_chunk_sent)
            })
            .collect()
    }

    fn chunk_is_on_packet_tracked_border(
        view: PlayerChunkView,
        chunk: ChunkPos,
        is_chunk_sent: &impl Fn(ChunkPos) -> bool,
    ) -> bool {
        if !Self::chunk_is_packet_tracked(view, chunk, is_chunk_sent) {
            return false;
        }

        for dx in -1..=1 {
            for dz in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }

                let neighbor = ChunkPos::new(chunk.0.x + dx, chunk.0.y + dz);
                if !Self::chunk_is_packet_tracked(view, neighbor, is_chunk_sent) {
                    return true;
                }
            }
        }

        false
    }

    fn chunk_is_packet_tracked(
        view: PlayerChunkView,
        chunk: ChunkPos,
        is_chunk_sent: &impl Fn(ChunkPos) -> bool,
    ) -> bool {
        view.contains(chunk) && is_chunk_sent(chunk)
    }

    /// Broadcasts a packet to players currently tracking an entity.
    pub fn broadcast_to_entity_trackers<P: ClientPacket>(
        &self,
        entity_id: i32,
        packet: P,
        exclude: Option<i32>,
    ) {
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            return;
        };
        self.broadcast_to_entity_trackers_encoded(entity_id, encoded, exclude);
    }

    /// Broadcasts a packet to players tracking an entity, excluding several players.
    pub fn broadcast_to_entity_trackers_except_many<P: ClientPacket>(
        &self,
        entity_id: i32,
        packet: P,
        excluded_player_ids: &[i32],
    ) {
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            return;
        };

        for player_id in self.entity_tracker.tracking_player_ids(entity_id) {
            if excluded_player_ids.contains(&player_id) {
                continue;
            }
            if let Some(player) = self.players.get_by_entity_id(player_id) {
                player.connection.send_encoded(encoded.clone());
            }
        }
    }

    /// Broadcasts an entity movement sync packet to players currently tracking an entity.
    pub fn broadcast_movement_sync_to_entity_trackers(
        &self,
        entity_id: i32,
        packet: EntityMovementSyncPacket,
        exclude: Option<i32>,
    ) {
        let Some(encoded) = self.encode_movement_sync_packet(packet) else {
            return;
        };
        self.broadcast_to_entity_trackers_encoded(entity_id, encoded, exclude);
    }

    /// Broadcasts an already-encoded packet to players currently tracking an entity.
    pub fn broadcast_to_entity_trackers_encoded(
        &self,
        entity_id: i32,
        packet: EncodedPacket,
        exclude: Option<i32>,
    ) {
        for player_id in self.entity_tracker.tracking_player_ids(entity_id) {
            if Some(player_id) == exclude {
                continue;
            }
            if let Some(player) = self.players.get_by_entity_id(player_id) {
                player.connection.send_encoded(packet.clone());
            }
        }
    }

    fn encode_movement_sync_packet(
        &self,
        packet: EntityMovementSyncPacket,
    ) -> Option<EncodedPacket> {
        let encoded = match packet {
            EntityMovementSyncPacket::Position(packet) => {
                EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
            }
            EntityMovementSyncPacket::PositionRotation(packet) => {
                EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
            }
            EntityMovementSyncPacket::Rotation(packet) => {
                EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
            }
            EntityMovementSyncPacket::HeadRotation(packet) => {
                EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
            }
            EntityMovementSyncPacket::PositionSync(packet) => {
                EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
            }
            EntityMovementSyncPacket::Velocity(packet) => {
                EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
            }
        };
        encoded.ok()
    }

    /// Saves all dirty chunks in this world to disk.
    ///
    /// This should be called during graceful shutdown.
    /// Returns the number of chunks saved.
    pub async fn save_all_chunks(&self) -> io::Result<usize> {
        self.chunk_map.save_all_chunks().await
    }

    /// Broadcasts block destruction progress to nearby players.
    ///
    /// Note: The packet is NOT sent to the player doing the breaking (matching vanilla).
    /// The breaking player sees progress through client-side prediction.
    ///
    /// # Arguments
    /// * `entity_id` - The entity ID of the player breaking the block
    /// * `pos` - The position of the block being broken
    /// * `progress` - The destruction progress (0-9), or -1 to clear
    #[expect(
        clippy::cast_sign_loss,
        reason = "value is clamped to -1..=9 before cast; -1 wraps intentionally to 255 as sentinel"
    )]
    pub fn broadcast_block_destruction(&self, entity_id: i32, pos: BlockPos, progress: i32) {
        let chunk = ChunkPos::new(
            SectionPos::block_to_section_coord(pos.x()),
            SectionPos::block_to_section_coord(pos.z()),
        );
        let packet = CBlockDestruction {
            id: entity_id,
            pos,
            progress: progress.clamp(-1, 9) as u8,
        };
        self.broadcast_to_nearby(chunk, packet, Some(entity_id));
    }

    /// Broadcasts a block entity update to all players tracking the chunk.
    ///
    /// This is used when block entity data changes (e.g., sign text updated).
    ///
    /// # Arguments
    /// * `pos` - The position of the block entity
    /// * `block_entity_type` - The type of block entity
    /// * `nbt` - The NBT data to send
    pub fn broadcast_block_entity_update(
        &self,
        pos: BlockPos,
        block_entity_type: BlockEntityTypeRef,
        nbt: NbtCompound,
    ) {
        use steel_protocol::packets::game::CBlockEntityData;
        use steel_utils::serial::OptionalNbt;

        let chunk = ChunkPos::new(
            SectionPos::block_to_section_coord(pos.x()),
            SectionPos::block_to_section_coord(pos.z()),
        );

        // Get the block entity type ID from the registry
        let type_id = block_entity_type.id();

        let packet = CBlockEntityData {
            pos,
            block_entity_type: type_id as i32,
            nbt: OptionalNbt(Some(nbt)),
        };

        self.broadcast_to_nearby(chunk, packet, None);
    }

    /// Drops an item stack at the given position with scatter behavior.
    ///
    /// Mirrors vanilla's `Containers.dropItemStack`. Splits large stacks into
    /// multiple item entities (10-30 items each) and scatters them with random
    /// positions and velocities.
    ///
    /// # Arguments
    /// * `pos` - The block position to drop the item at
    /// * `item` - The item stack to drop
    pub fn drop_item_stack(self: &Arc<Self>, pos: BlockPos, mut item: ItemStack) {
        use crate::entity::next_entity_id;
        use steel_registry::vanilla_entities;

        // Random velocity using triangle distribution (vanilla uses random.triangle)
        // Vanilla constant: 0.05F * Mth.SQRT_OF_TWO (sqrt(2) * 0.05 ≈ 0.1148...)
        const VELOCITY_SPREAD: f64 = 0.114_850_001_711_398_36;

        if item.is_empty() {
            return;
        }

        // Vanilla uses EntityType.ITEM dimensions for position calculation
        let item_width = f64::from(vanilla_entities::ITEM.dimensions.width);
        let center_range = 1.0 - item_width;
        let half_size = item_width / 2.0;

        // Keep spawning item entities until the stack is empty
        // Vanilla splits stacks into 10-30 items each
        while !item.is_empty() {
            // Split off 10-30 items (or remaining if less)
            let split_count = (rand::random::<u32>() % 21 + 10) as i32;
            let split_stack = item.split(split_count);

            if split_stack.is_empty() {
                break;
            }

            // Random position within the block (vanilla logic)
            let x = f64::from(pos.x()).floor() + rand::random::<f64>() * center_range + half_size;
            let y = f64::from(pos.y()).floor() + rand::random::<f64>() * center_range;
            let z = f64::from(pos.z()).floor() + rand::random::<f64>() * center_range + half_size;

            // triangle(mode, deviation) produces values centered around mode with spread of deviation
            let vx = triangle_random(0.0, VELOCITY_SPREAD);
            let vy = triangle_random(0.2, VELOCITY_SPREAD);
            let vz = triangle_random(0.0, VELOCITY_SPREAD);

            let entity_id = next_entity_id();
            let entity = Arc::new(ItemEntity::with_item_and_velocity(
                &vanilla_entities::ITEM,
                entity_id,
                DVec3::new(x, y, z),
                split_stack,
                DVec3::new(vx, vy, vz),
                Arc::downgrade(self),
            ));
            entity.set_default_pickup_delay();
            if let Err(error) = self.try_add_entity(entity) {
                log::warn!("Failed to drop item stack entity: {error}");
            }
        }
    }

    /// Checks if a ray intersects with a block's selection box.
    pub fn ray_outline_check(
        &self,
        block_pos: BlockPos,
        from: DVec3,
        to: DVec3,
    ) -> (bool, Option<Direction>) {
        let state = self.get_block_state(block_pos);
        let shape = state.get_outline_shape_at(block_pos);

        match Self::clip_shape(block_pos, from, to, shape) {
            Some(hit) => (true, Some(hit.direction)),
            None => (false, None),
        }
    }

    /// Performs a vanilla-style block/fluid clip in the world.
    #[must_use]
    pub fn clip(
        &self,
        start_pos: DVec3,
        end_pos: DVec3,
        block_shape: ClipBlockShape,
        fluid: ClipFluid,
    ) -> ClipHitResult {
        if start_pos == end_pos {
            return Self::clip_miss(start_pos, end_pos);
        }

        let adjust = -1.0e-7f64;
        let to = end_pos.lerp(start_pos, adjust);
        let from = start_pos.lerp(end_pos, adjust);

        let mut block = BlockPos::new(
            from.x.floor() as i32,
            from.y.floor() as i32,
            from.z.floor() as i32,
        );

        if let Some(hit) = self.clip_block_and_fluid(block, start_pos, end_pos, block_shape, fluid)
        {
            return hit;
        }

        let difference = to - from;

        let step = difference.signum().as_ivec3();

        let delta = DVec3::new(
            if step.x == 0 {
                f64::MAX
            } else {
                (f64::from(step.x)) / difference.x
            },
            if step.y == 0 {
                f64::MAX
            } else {
                (f64::from(step.y)) / difference.y
            },
            if step.z == 0 {
                f64::MAX
            } else {
                (f64::from(step.z)) / difference.z
            },
        );

        let mut next = DVec3::new(
            delta.x
                * (if step.x > 0 {
                    1.0 - (from.x - from.x.floor())
                } else {
                    from.x - from.x.floor()
                }),
            delta.y
                * (if step.y > 0 {
                    1.0 - (from.y - from.y.floor())
                } else {
                    from.y - from.y.floor()
                }),
            delta.z
                * (if step.z > 0 {
                    1.0 - (from.z - from.z.floor())
                } else {
                    from.z - from.z.floor()
                }),
        );

        while next.x <= 1.0 || next.y <= 1.0 || next.z <= 1.0 {
            if next.x < next.y && next.x < next.z {
                block.0.x += step.x;
                next.x += delta.x;
            } else if next.y < next.z {
                block.0.y += step.y;
                next.y += delta.y;
            } else {
                block.0.z += step.z;
                next.z += delta.z;
            }

            if let Some(hit) =
                self.clip_block_and_fluid(block, start_pos, end_pos, block_shape, fluid)
            {
                return hit;
            }
        }

        Self::clip_miss(start_pos, end_pos)
    }

    fn clip_block_and_fluid(
        &self,
        pos: BlockPos,
        from: DVec3,
        to: DVec3,
        block_shape: ClipBlockShape,
        fluid: ClipFluid,
    ) -> Option<ClipHitResult> {
        let state = self.get_block_state(pos);
        let block_result = Self::clip_shape(
            pos,
            from,
            to,
            self.clip_block_shape(state, pos, block_shape),
        )
        .map(|hit| Self::clip_with_interaction_override(pos, from, to, state, hit));
        let fluid_result = self.clip_fluid_shape(pos, from, to, state, fluid);

        match (block_result, fluid_result) {
            (Some(block_hit), Some(fluid_hit)) => {
                let block_distance = from.distance_squared(block_hit.location);
                let fluid_distance = from.distance_squared(fluid_hit.location);
                if block_distance <= fluid_distance {
                    Some(block_hit)
                } else {
                    Some(fluid_hit)
                }
            }
            (Some(hit), None) | (None, Some(hit)) => Some(hit),
            (None, None) => None,
        }
    }

    fn clip_with_interaction_override(
        pos: BlockPos,
        from: DVec3,
        to: DVec3,
        state: BlockStateId,
        block_hit: ClipHitResult,
    ) -> ClipHitResult {
        let Some(override_hit) =
            Self::clip_shape(pos, from, to, state.get_interaction_shape_at(pos))
        else {
            return block_hit;
        };

        if from.distance_squared(override_hit.location) < from.distance_squared(block_hit.location)
        {
            ClipHitResult {
                direction: override_hit.direction,
                ..block_hit
            }
        } else {
            block_hit
        }
    }

    fn clip_block_shape(
        &self,
        state: BlockStateId,
        pos: BlockPos,
        shape: ClipBlockShape,
    ) -> OffsetVoxelShape {
        match shape {
            ClipBlockShape::Collider => state.get_collision_shape_at(pos),
            ClipBlockShape::Outline => state.get_outline_shape_at(pos),
            ClipBlockShape::Visual => state.get_visual_shape_at(pos),
            ClipBlockShape::FallDamageResetting { entity_is_player } => {
                OffsetVoxelShape::without_offset(
                    self.fall_damage_resetting_shape(state, entity_is_player),
                )
            }
        }
    }

    fn fall_damage_resetting_shape(
        &self,
        state: BlockStateId,
        entity_is_player: bool,
    ) -> VoxelShape {
        let block = state.get_block();
        if block.has_tag(&BlockTag::FALL_DAMAGE_RESETTING) {
            return VoxelShape::FULL_BLOCK;
        }

        if !entity_is_player {
            return VoxelShape::EMPTY;
        }

        if block == &vanilla_blocks::END_GATEWAY || block == &vanilla_blocks::END_PORTAL {
            return VoxelShape::FULL_BLOCK;
        }

        if block == &vanilla_blocks::NETHER_PORTAL
            && self.get_game_rule(&PLAYERS_NETHER_PORTAL_DEFAULT_DELAY) == GameRuleValue::Int(0)
        {
            return VoxelShape::FULL_BLOCK;
        }

        VoxelShape::EMPTY
    }

    fn clip_fluid_shape(
        &self,
        pos: BlockPos,
        from: DVec3,
        to: DVec3,
        state: BlockStateId,
        fluid: ClipFluid,
    ) -> Option<ClipHitResult> {
        let fluid_state = state.get_fluid_state();
        let can_pick = match fluid {
            ClipFluid::None => false,
            ClipFluid::SourceOnly => fluid_state.is_source(),
            ClipFluid::Any => !fluid_state.is_empty(),
            ClipFluid::Water => fluid_state.is_water(),
        };
        if !can_pick {
            return None;
        }

        let height = self.fluid_clip_height(pos, fluid_state);
        Self::clip_local_aabb(
            pos,
            from,
            to,
            BlockLocalAabb::new(0.0, 0.0, 0.0, 1.0, height, 1.0),
        )
    }

    fn fluid_clip_height(&self, pos: BlockPos, fluid_state: FluidState) -> f64 {
        let above_fluid = self.get_block_state(pos.above()).get_fluid_state();
        Self::fluid_clip_height_from_above(fluid_state, above_fluid)
    }

    fn fluid_clip_height_from_above(fluid_state: FluidState, above_fluid: FluidState) -> f64 {
        if FLUID_BEHAVIORS
            .get_behavior(fluid_state.fluid_id)
            .is_same(above_fluid.fluid_id)
        {
            1.0
        } else {
            f64::from(fluid_state.own_height())
        }
    }

    fn clip_shape(
        block_pos: BlockPos,
        from: DVec3,
        to: DVec3,
        shape: OffsetVoxelShape,
    ) -> Option<ClipHitResult> {
        if shape.is_empty() {
            return None;
        }

        if (to - from).length_squared() < 1.0e-7 {
            return None;
        }

        let block_vec = DVec3::new(
            f64::from(block_pos.x()),
            f64::from(block_pos.y()),
            f64::from(block_pos.z()),
        );
        let inside_test_point = from + (to - from) * 0.001;
        if Self::shape_contains_world_point(shape, block_vec, inside_test_point) {
            return Some(ClipHitResult {
                location: inside_test_point,
                direction: Self::approximate_nearest_direction(to - from).opposite(),
                block_pos,
                miss: false,
                inside: true,
            });
        }

        let mut closest: Option<(f64, Direction)> = None;

        for shape in shape.iter() {
            let world_min = DVec3::new(shape.min_x(), shape.min_y(), shape.min_z()) + block_vec;
            let world_max = DVec3::new(shape.max_x(), shape.max_y(), shape.max_z()) + block_vec;

            if let Some(hit) = Self::intersects_aabb_with_t(from, to, world_min, world_max)
                && hit.0 > 0.0
                && hit.0 < 1.0
                && closest.is_none_or(|(best_t, _)| hit.0 < best_t)
            {
                closest = Some(hit);
            }
        }

        closest.map(|(t, direction)| ClipHitResult {
            location: from + (to - from) * t,
            direction,
            block_pos,
            miss: false,
            inside: false,
        })
    }

    fn clip_local_aabb(
        block_pos: BlockPos,
        from: DVec3,
        to: DVec3,
        aabb: BlockLocalAabb,
    ) -> Option<ClipHitResult> {
        if aabb.is_empty() {
            return None;
        }

        if (to - from).length_squared() < 1.0e-7 {
            return None;
        }

        let block_vec = DVec3::new(
            f64::from(block_pos.x()),
            f64::from(block_pos.y()),
            f64::from(block_pos.z()),
        );
        let inside_test_point = from + (to - from) * 0.001;
        if Self::local_aabb_contains_world_point(aabb, block_vec, inside_test_point) {
            return Some(ClipHitResult {
                location: inside_test_point,
                direction: Self::approximate_nearest_direction(to - from).opposite(),
                block_pos,
                miss: false,
                inside: true,
            });
        }

        let world_min = DVec3::new(aabb.min_x(), aabb.min_y(), aabb.min_z()) + block_vec;
        let world_max = DVec3::new(aabb.max_x(), aabb.max_y(), aabb.max_z()) + block_vec;
        Self::intersects_aabb_with_t(from, to, world_min, world_max).and_then(|(t, direction)| {
            if t > 0.0 && t < 1.0 {
                Some(ClipHitResult {
                    location: from + (to - from) * t,
                    direction,
                    block_pos,
                    miss: false,
                    inside: false,
                })
            } else {
                None
            }
        })
    }

    fn shape_contains_world_point(shape: OffsetVoxelShape, block_vec: DVec3, point: DVec3) -> bool {
        shape
            .iter()
            .any(|aabb| Self::local_aabb_contains_world_point(aabb, block_vec, point))
    }

    fn local_aabb_contains_world_point(
        aabb: BlockLocalAabb,
        block_vec: DVec3,
        point: DVec3,
    ) -> bool {
        let local = point - block_vec;
        !aabb.is_empty()
            && local.x >= aabb.min_x()
            && local.x <= aabb.max_x()
            && local.y >= aabb.min_y()
            && local.y <= aabb.max_y()
            && local.z >= aabb.min_z()
            && local.z <= aabb.max_z()
    }

    fn clip_miss(from: DVec3, to: DVec3) -> ClipHitResult {
        ClipHitResult {
            location: to,
            direction: Self::approximate_nearest_direction(from - to),
            block_pos: BlockPos::from(to),
            miss: true,
            inside: false,
        }
    }

    fn approximate_nearest_direction(vector: DVec3) -> Direction {
        let mut result = Direction::North;
        let mut highest_dot = 0.0;
        for direction in [
            Direction::Down,
            Direction::Up,
            Direction::North,
            Direction::South,
            Direction::West,
            Direction::East,
        ] {
            let dot = vector.dot(direction.offset_vec().as_dvec3());
            if dot > highest_dot {
                highest_dot = dot;
                result = direction;
            }
        }
        result
    }

    /// Ray-AABB intersection returning the entry t-parameter and the hit face.
    ///
    /// Returns `Some((tmin, direction))` where `tmin` is the ray parameter at entry
    /// and `direction` is the face normal pointing away from the hit surface.
    /// Returns `None` if the AABB is missed or entirely behind the ray origin.
    ///
    /// Used internally by [`ray_outline_check`] to pick the *closest* hit across
    /// a multi-box voxel shape, matching vanilla's `VoxelShape.clip()` behavior.
    fn intersects_aabb_with_t(
        start: DVec3,
        end: DVec3,
        min: DVec3,
        max: DVec3,
    ) -> Option<(f64, Direction)> {
        let dir = end - start;

        let mut tmin = f64::NEG_INFINITY;
        let mut tmax = f64::INFINITY;
        let mut hit_dir = None;

        macro_rules! slab {
            ($start:expr, $dir:expr, $min:expr, $max:expr, $neg:expr, $pos:expr) => {{
                if $dir.abs() < 1e-8 {
                    if $start < $min || $start > $max {
                        return None;
                    }
                } else {
                    let inv = 1.0 / $dir;
                    let mut t1 = ($min - $start) * inv;
                    let mut t2 = ($max - $start) * inv;

                    let dir_hit = if t1 > t2 {
                        std::mem::swap(&mut t1, &mut t2);
                        $pos
                    } else {
                        $neg
                    };

                    if t1 > tmin {
                        tmin = t1;
                        hit_dir = Some(dir_hit);
                    }

                    tmax = tmax.min(t2);
                    if tmin > tmax {
                        return None;
                    }
                }
            }};
        }

        slab!(
            start.x,
            dir.x,
            min.x,
            max.x,
            Direction::West,
            Direction::East
        );
        slab!(start.y, dir.y, min.y, max.y, Direction::Down, Direction::Up);
        slab!(
            start.z,
            dir.z,
            min.z,
            max.z,
            Direction::North,
            Direction::South
        );

        if tmax < 0.0 {
            None
        } else {
            hit_dir.map(|d| (tmin, d))
        }
    }

    /// Performs a raytrace in the world.
    ///
    /// Adapted from Pumpkin project.
    pub fn raytrace<F>(
        &self,
        start_pos: DVec3,
        end_pos: DVec3,
        hit_check: F,
    ) -> (Option<BlockPos>, Option<Direction>)
    where
        F: Fn(BlockPos, &Self) -> RaytraceAction,
    {
        if start_pos == end_pos {
            return (None, None);
        }

        let adjust = -1.0e-7f64;
        let to = end_pos.lerp(start_pos, adjust);
        let from = start_pos.lerp(end_pos, adjust);

        let mut block = BlockPos::new(
            from.x.floor() as i32,
            from.y.floor() as i32,
            from.z.floor() as i32,
        );

        match hit_check(block, self) {
            RaytraceAction::ImmediateHit => return (Some(block), None),
            RaytraceAction::CheckShape => {
                let (hit, face) = self.ray_outline_check(block, start_pos, end_pos);
                if hit {
                    return (Some(block), face);
                }
            }
            RaytraceAction::Pass => {}
        }

        let difference = to - from;

        let step = difference.signum().as_ivec3();

        let delta = DVec3::new(
            if step.x == 0 {
                f64::MAX
            } else {
                (f64::from(step.x)) / difference.x
            },
            if step.y == 0 {
                f64::MAX
            } else {
                (f64::from(step.y)) / difference.y
            },
            if step.z == 0 {
                f64::MAX
            } else {
                (f64::from(step.z)) / difference.z
            },
        );

        let mut next = DVec3::new(
            delta.x
                * (if step.x > 0 {
                    1.0 - (from.x - from.x.floor())
                } else {
                    from.x - from.x.floor()
                }),
            delta.y
                * (if step.y > 0 {
                    1.0 - (from.y - from.y.floor())
                } else {
                    from.y - from.y.floor()
                }),
            delta.z
                * (if step.z > 0 {
                    1.0 - (from.z - from.z.floor())
                } else {
                    from.z - from.z.floor()
                }),
        );

        while next.x <= 1.0 || next.y <= 1.0 || next.z <= 1.0 {
            // Vanilla parity: traverseBlocks tie-breaking — Z wins on any tie.
            // X wins only when strictly less than both Y and Z.
            // Y wins only when strictly less than both X and Z.
            // Everything else (including all ties) goes to Z.
            let block_direction = if next.x < next.y && next.x < next.z {
                block.0.x += step.x;
                next.x += delta.x;
                if step.x > 0 {
                    Direction::West
                } else {
                    Direction::East
                }
            } else if next.y < next.x && next.y < next.z {
                block.0.y += step.y;
                next.y += delta.y;
                if step.y > 0 {
                    Direction::Down
                } else {
                    Direction::Up
                }
            } else {
                block.0.z += step.z;
                next.z += delta.z;
                if step.z > 0 {
                    Direction::North
                } else {
                    Direction::South
                }
            };

            match hit_check(block, self) {
                RaytraceAction::ImmediateHit => {
                    return (Some(block), Some(block_direction));
                }
                RaytraceAction::CheckShape => {
                    let (hit, face) = self.ray_outline_check(block, start_pos, end_pos);
                    if hit {
                        return (Some(block), face);
                    }
                }
                RaytraceAction::Pass => {}
            }
        }

        (None, None)
    }
    /// Broadcasts a level event to nearby players within 64 blocks.
    ///
    /// Level events trigger sounds, particles, and animations on the client.
    /// See `steel_registry::level_events` for available event type constants.
    ///
    /// # Arguments
    /// * `event_type` - The event type ID from `steel_registry::level_events`
    /// * `pos` - The position where the event occurs
    /// * `data` - Event-specific data (e.g., block state ID for block destruction)
    /// * `exclude` - Optional entity ID to exclude from receiving the event
    pub fn level_event(&self, event_type: i32, pos: BlockPos, data: i32, exclude: Option<i32>) {
        const MAX_DISTANCE_SQ: f64 = 64.0 * 64.0;

        let chunk = ChunkPos::new(
            SectionPos::block_to_section_coord(pos.x()),
            SectionPos::block_to_section_coord(pos.z()),
        );
        let packet = CLevelEvent::new(event_type, pos, data, false);
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            log::warn!("Failed to encode level event packet");
            return;
        };

        // Get players tracking this chunk, then filter by 64-block distance
        let event_pos = (
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 0.5,
            f64::from(pos.z()) + 0.5,
        );

        for entity_id in self.player_area_map.get_tracking_players(chunk) {
            // Skip excluded player (they hear the effect client-side)
            if exclude == Some(entity_id) {
                continue;
            }
            if let Some(player) = self.players.get_by_entity_id(entity_id) {
                let player_pos = player.position();
                let dx = player_pos.x - event_pos.0;
                let dy = player_pos.y - event_pos.1;
                let dz = player_pos.z - event_pos.2;
                let dist_sq = dx * dx + dy * dy + dz * dz;

                if dist_sq <= MAX_DISTANCE_SQ {
                    player.connection.send_encoded(encoded.clone());
                }
            }
        }
    }

    /// Broadcasts a global level event to all players in the world.
    ///
    /// Unlike `level_event`, this sends the event to all players regardless of distance.
    /// Used for events like the ender dragon death or wither spawn.
    ///
    /// # Arguments
    /// * `event_type` - The event type ID from `steel_registry::level_events`
    /// * `pos` - The position where the event occurs
    /// * `data` - Event-specific data
    pub fn global_level_event(&self, event_type: i32, pos: BlockPos, data: i32) {
        let packet = CLevelEvent::new(event_type, pos, data, true);
        self.players.iter_players(|_, player| {
            player.send_packet(packet.clone());
            true
        });
    }

    /// Broadcasts block destruction particles and sound for a destroyed block.
    ///
    /// This is a convenience method that sends the `PARTICLES_DESTROY_BLOCK` level event.
    ///
    /// # Arguments
    /// * `pos` - The position of the destroyed block
    /// * `block_state_id` - The block state ID of the destroyed block
    /// * `exclude` - Optional entity ID to exclude from receiving the event
    pub fn destroy_block_effect(&self, pos: BlockPos, block_state_id: u32, exclude: Option<i32>) {
        self.level_event(
            level_events::PARTICLES_DESTROY_BLOCK,
            pos,
            block_state_id as i32,
            exclude,
        );
    }

    /// Destroys a block at the given position, optionally dropping its loot.
    ///
    /// Sends destruction particles (skipping fire blocks), optionally drops
    /// resources via loot table, then replaces with air.
    ///
    /// Defaults to recursion limit of 512
    pub fn destroy_block(self: &Arc<Self>, pos: BlockPos, drop_items: bool) -> bool {
        self.destroy_block_with_limit(pos, drop_items, 512)
    }

    /// Destroys a block with an entity source for game-event context.
    pub fn destroy_block_by_entity(
        self: &Arc<Self>,
        pos: BlockPos,
        drop_items: bool,
        entity: &dyn Entity,
    ) -> bool {
        self.destroy_block_with_limit_and_entity(pos, drop_items, 512, Some(entity))
    }

    /// Destroys a block at the given position, optionally dropping its loot.
    ///
    /// Sends destruction particles (skipping fire blocks), optionally drops
    /// resources via loot table, then replaces with air.
    pub fn destroy_block_with_limit(
        self: &Arc<Self>,
        pos: BlockPos,
        drop_items: bool,
        recursion_left: i32,
    ) -> bool {
        self.destroy_block_with_limit_and_entity(pos, drop_items, recursion_left, None)
    }

    fn destroy_block_with_limit_and_entity(
        self: &Arc<Self>,
        pos: BlockPos,
        drop_items: bool,
        recursion_left: i32,
        entity: Option<&dyn Entity>,
    ) -> bool {
        let state = self.get_block_state(pos);
        if state.is_air() {
            return false;
        }

        let block = state.get_block();
        let is_fire = block == &vanilla_blocks::FIRE || block == &vanilla_blocks::SOUL_FIRE;
        if !is_fire {
            self.destroy_block_effect(pos, u32::from(state.0), None);
        }

        if drop_items {
            self.drop_resources(state, pos);
            // TODO: block entity and entity drops
        }

        // Vanilla parity: fluidState.createLegacyBlock() — breaking a waterlogged
        // block leaves water behind instead of air.
        let replacement = fluid_state_to_block(state.get_fluid_state());
        let destroyed =
            self.set_block_with_limit(pos, replacement, UpdateFlags::UPDATE_ALL, recursion_left);
        if destroyed {
            self.game_event(
                &vanilla_game_events::BLOCK_DESTROY,
                pos,
                &GameEventContext::new(entity, Some(state)),
            );
        }
        destroyed
    }

    /// Drops the loot for a block using its loot table.
    ///
    /// This is the no-tool/no-entity overload. Player block breaking uses
    /// `block_breaking::drop_block_loot` which includes tool context for
    /// fortune/silk touch.
    // TODO: `spawnAfterBreak` (XP orbs for ores) not called yet.
    // TODO: block entity and entity drops
    pub fn drop_resources(self: &Arc<Self>, state: BlockStateId, pos: BlockPos) {
        let block = state.get_block();
        let loot_key = steel_utils::Identifier::vanilla(format!("blocks/{}", block.key.path));

        let Some(loot_table) = REGISTRY.loot_tables.by_key(&loot_key) else {
            return;
        };

        let mut rng = rand::rng();
        let mut ctx = LootContext::new(&mut rng)
            .with_block_state(state)
            .with_origin(f64::from(pos.x()), f64::from(pos.y()), f64::from(pos.z()));

        let drops = loot_table.get_random_items(&mut ctx);
        for item in drops {
            if !item.is_empty() {
                self.pop_resource(pos, item);
            }
        }
    }

    /// Broadcasts a block event to nearby players within 64 blocks.
    ///
    /// Block events are used for special block behaviors like pistons, note blocks,
    /// chests, and bells. Each block type interprets the parameters differently.
    ///
    /// # Arguments
    /// * `pos` - The position of the block
    /// * `block` - The block reference
    /// * `action_id` - The action ID (block-specific meaning)
    /// * `action_param` - The action parameter (block-specific meaning)
    pub fn block_event(&self, pos: BlockPos, block: BlockRef, action_id: u8, action_param: u8) {
        const MAX_DISTANCE_SQ: f64 = 64.0 * 64.0;

        let block_id = block.id() as i32;

        let chunk = ChunkPos::new(
            SectionPos::block_to_section_coord(pos.x()),
            SectionPos::block_to_section_coord(pos.z()),
        );
        let packet = CBlockEvent::new(pos, action_id, action_param, block_id);
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            log::warn!("Failed to encode block event packet");
            return;
        };

        // Get players tracking this chunk, then filter by 64-block distance
        let event_pos = (
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 0.5,
            f64::from(pos.z()) + 0.5,
        );

        for entity_id in self.player_area_map.get_tracking_players(chunk) {
            if let Some(player) = self.players.get_by_entity_id(entity_id) {
                let player_pos = player.position();
                let dx = player_pos.x - event_pos.0;
                let dy = player_pos.y - event_pos.1;
                let dz = player_pos.z - event_pos.2;
                let dist_sq = dx * dx + dy * dy + dz * dz;

                if dist_sq <= MAX_DISTANCE_SQ {
                    player.connection.send_encoded(encoded.clone());
                }
            }
        }
    }

    /// Plays a sound at a specific position, broadcasting to nearby players.
    ///
    /// The sound is sent to all players within 64 blocks of the position,
    /// except for the excluded player (if any). The excluded player is typically
    /// the one who triggered the sound, as they hear it client-side.
    ///
    /// # Arguments
    /// * `sound` - The sound event to play
    /// * `source` - The sound source category
    /// * `pos` - The block position (sound plays at center of block)
    /// * `volume` - Volume multiplier (1.0 = normal)
    /// * `pitch` - Pitch multiplier (1.0 = normal)
    /// * `exclude` - Optional entity ID to exclude from receiving the sound
    pub fn play_sound(
        &self,
        sound: SoundEventRef,
        source: SoundSource,
        pos: BlockPos,
        volume: f32,
        pitch: f32,
        exclude: Option<i32>,
    ) {
        self.play_sound_at(
            sound,
            source,
            DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 0.5,
                f64::from(pos.z()) + 0.5,
            ),
            volume,
            pitch,
            exclude,
        );
    }

    /// Plays a sound at an exact world position, broadcasting to nearby players.
    pub fn play_sound_at(
        &self,
        sound: SoundEventRef,
        source: SoundSource,
        pos: DVec3,
        volume: f32,
        pitch: f32,
        exclude: Option<i32>,
    ) {
        const MAX_DISTANCE_SQ: f64 = 64.0 * 64.0;

        let chunk = ChunkPos::new(
            SectionPos::block_to_section_coord(pos.x.floor() as i32),
            SectionPos::block_to_section_coord(pos.z.floor() as i32),
        );

        // Generate a random seed for sound variations
        let seed = rand::random::<i64>();

        let packet = CSound::new(sound, source, pos, volume, pitch, seed);
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            log::warn!("Failed to encode sound packet");
            return;
        };

        // Get players tracking this chunk, then filter by 64-block distance
        for entity_id in self.player_area_map.get_tracking_players(chunk) {
            // Skip excluded player (they hear the sound client-side)
            if exclude == Some(entity_id) {
                continue;
            }
            if let Some(player) = self.players.get_by_entity_id(entity_id) {
                let player_pos = player.position();
                let dx = player_pos.x - pos.x;
                let dy = player_pos.y - pos.y;
                let dz = player_pos.z - pos.z;
                let dist_sq = dx * dx + dy * dy + dz * dz;

                if dist_sq <= MAX_DISTANCE_SQ {
                    player.connection.send_encoded(encoded.clone());
                }
            }
        }
    }

    /// Plays a block sound at a specific position.
    ///
    /// Convenience method that uses the BLOCKS sound source and applies
    /// the sound type's volume and pitch modifiers.
    ///
    /// # Arguments
    /// * `sound` - The sound event to play
    /// * `pos` - The block position
    /// * `volume` - Base volume (typically from `SoundType`)
    /// * `pitch` - Base pitch (typically from `SoundType`)
    /// * `exclude` - Optional entity ID to exclude from receiving the sound
    pub fn play_block_sound(
        &self,
        sound: SoundEventRef,
        pos: BlockPos,
        volume: f32,
        pitch: f32,
        exclude: Option<i32>,
    ) {
        self.play_sound(sound, SoundSource::Blocks, pos, volume, pitch, exclude);
    }

    /// Returns the runtime entity manager.
    #[must_use]
    pub const fn entity_manager(&self) -> &WorldEntityManager {
        &self.entity_manager
    }

    /// Returns the entity tracker for managing player-entity visibility.
    #[must_use]
    pub const fn entity_tracker(&self) -> &EntityTracker {
        &self.entity_tracker
    }

    fn attach_managed_entity_callback(self: &Arc<Self>, entity: &SharedEntity) {
        let callback = Arc::new(EntityChunkCallback::new(entity.id(), Arc::downgrade(self)));
        entity.set_level_callback(callback);
    }

    pub(crate) fn add_entity_to_tracker(self: &Arc<Self>, entity: &SharedEntity) {
        self.entity_tracker.add(
            entity,
            |chunk| self.get_packet_tracking_players(chunk),
            |id| self.players.get_by_entity_id(id),
        );
        self.track_navigating_mob(entity);
    }

    pub(crate) fn remove_entity_from_tracker(&self, entity_id: i32) {
        self.entity_tracker.remove(entity_id, |player_id| {
            self.players.get_by_entity_id(player_id)
        });
        self.untrack_navigating_mob(entity_id);
    }

    pub(crate) fn apply_entity_lifecycle_changes(
        self: &Arc<Self>,
        changes: EntityLifecycleChanges,
    ) {
        for entity in changes.tracking_stopped {
            self.remove_entity_from_tracker(entity.id());
        }
        for entity in changes.tracking_started {
            self.add_entity_to_tracker(&entity);
        }
    }

    fn track_navigating_mob(&self, entity: &SharedEntity) {
        self.navigating_mobs.track(entity);
    }

    fn untrack_navigating_mob(&self, entity_id: i32) {
        self.navigating_mobs.untrack(entity_id);
    }

    pub(crate) fn register_loaded_entity(
        self: &Arc<Self>,
        entity: SharedEntity,
    ) -> Result<(), AddEntityError> {
        let lifecycle = self
            .entity_manager
            .add_live_entity(entity.clone(), EntityOwnership::ManagerOwned)?;
        self.attach_managed_entity_callback(&entity);
        self.apply_entity_lifecycle_changes(lifecycle);
        Ok(())
    }

    pub(crate) fn register_loaded_entity_tree(
        self: &Arc<Self>,
        entities: &[SharedEntity],
    ) -> Result<(), AddEntityError> {
        let lifecycle = self
            .entity_manager
            .add_live_entity_tree(entities, EntityOwnership::ManagerOwned)?;
        for entity in entities {
            self.attach_managed_entity_callback(entity);
        }
        self.apply_entity_lifecycle_changes(lifecycle);
        Ok(())
    }

    pub(crate) fn register_loaded_chunk_entities(
        self: &Arc<Self>,
        source_chunk: ChunkPos,
        persisted_status: ChunkStatus,
        entities: Vec<SharedEntity>,
    ) {
        for tree in Self::loaded_entity_trees(entities) {
            let Some(root) = tree.first() else {
                continue;
            };
            let root_id = root.id();
            let root_uuid = root.uuid();
            let root_type = root.entity_type();
            let root_pos = root.position();
            let root_chunk = ChunkPos::from_entity_pos(root_pos);
            let mut dirty_chunks = FxHashSet::default();
            for entity in &tree {
                let entity_chunk = ChunkPos::from_entity_pos(entity.position());
                if entity_chunk != source_chunk {
                    dirty_chunks.insert(source_chunk);
                    dirty_chunks.insert(entity_chunk);
                }
            }

            if let Err(error) = self.register_loaded_entity_tree(&tree) {
                tracing::warn!(
                    source_chunk = ?source_chunk,
                    ?persisted_status,
                    root_id,
                    uuid = ?root_uuid,
                    entity_type = ?root_type.key,
                    position = ?root_pos,
                    entity_chunk = ?root_chunk,
                    entity_count = tree.len(),
                    "Discarding loaded chunk entity tree that could not be registered: {error}; source_chunk={source_chunk:?}, persisted_status={persisted_status:?}, root_id={root_id}, uuid={root_uuid}, entity_type={:?}, position={root_pos:?}, entity_chunk={root_chunk:?}, entity_count={}",
                    root_type.key,
                    tree.len(),
                );
                Self::discard_loaded_entity_tree(&tree);
                self.mark_chunk_dirty(source_chunk);
                continue;
            }

            for chunk in dirty_chunks {
                self.mark_chunk_dirty(chunk);
            }
        }
    }

    fn loaded_entity_trees(entities: Vec<SharedEntity>) -> Vec<Vec<SharedEntity>> {
        let mut seen = FxHashSet::default();
        let mut trees = Vec::new();

        for entity in &entities {
            if entity.is_passenger() {
                continue;
            }
            let mut tree = Vec::new();
            Self::collect_loaded_entity_tree(entity, &mut seen, &mut tree);
            if !tree.is_empty() {
                trees.push(tree);
            }
        }

        for entity in &entities {
            if seen.contains(&entity.id()) {
                continue;
            }
            let mut tree = Vec::new();
            Self::collect_loaded_entity_tree(entity, &mut seen, &mut tree);
            if !tree.is_empty() {
                trees.push(tree);
            }
        }

        trees
    }

    fn collect_loaded_entity_tree(
        entity: &SharedEntity,
        seen: &mut FxHashSet<i32>,
        tree: &mut Vec<SharedEntity>,
    ) {
        if !seen.insert(entity.id()) {
            return;
        }
        tree.push(Arc::clone(entity));
        for passenger in entity.passengers() {
            Self::collect_loaded_entity_tree(&passenger, seen, tree);
        }
    }

    fn discard_loaded_entity_tree(entities: &[SharedEntity]) {
        for entity in entities {
            entity.set_removed(RemovalReason::Discarded);
        }
    }

    pub(crate) fn has_full_chunk(&self, chunk_pos: ChunkPos) -> bool {
        self.chunk_map
            .with_full_chunk(chunk_pos, |chunk| chunk.as_full().is_some())
            .unwrap_or(false)
    }

    /// Adds a runtime entity to the world.
    pub fn try_add_entity(self: &Arc<Self>, entity: SharedEntity) -> Result<(), AddEntityError> {
        let chunk_pos = ChunkPos::from_entity_pos(entity.position());
        if !self.has_full_chunk(chunk_pos) {
            return Err(AddEntityError::ChunkNotLoaded {
                entity_id: entity.id(),
                chunk: chunk_pos,
            });
        }
        self.register_loaded_entity(entity)?;
        self.mark_chunk_dirty(chunk_pos);
        Ok(())
    }

    pub(crate) fn on_entity_chunk_loaded(self: &Arc<Self>, pos: ChunkPos) {
        // Runtime entity membership follows retained chunk holders, so it
        // starts at Empty rather than waiting for full LevelChunk readiness.
        // Durable entity persistence still starts at StructureStarts; entities
        // in lower-status chunks can be lost if those chunks unload first.
        let result = self.entity_manager.on_chunk_loaded(pos);
        if result.needs_save {
            self.mark_chunk_dirty(pos);
        }
        for entity in result.restored {
            self.attach_managed_entity_callback(&entity);
        }
        self.apply_entity_lifecycle_changes(EntityLifecycleChanges {
            tracking_started: result.tracking_started,
            tracking_stopped: Vec::new(),
            ticking_started: result.ticking_started,
            ticking_stopped: Vec::new(),
        });
    }

    pub(crate) fn update_entity_chunk_visibility(
        self: &Arc<Self>,
        pos: ChunkPos,
        visibility: EntityVisibility,
    ) {
        let changes = self.entity_manager.update_chunk_visibility(pos, visibility);
        self.apply_entity_lifecycle_changes(changes);
    }

    pub(crate) fn on_entity_chunk_unload_start(self: &Arc<Self>, pos: ChunkPos) {
        let result = self.entity_manager.begin_chunk_unload(pos);
        self.apply_entity_lifecycle_changes(EntityLifecycleChanges {
            tracking_started: Vec::new(),
            tracking_stopped: result.tracking_stopped,
            ticking_started: Vec::new(),
            ticking_stopped: result.ticking_stopped,
        });
        for entity in result.retained {
            let entity_id = entity.id();
            entity.set_level_callback(Arc::new(InactiveEntityCallback::new(entity_id)));
        }
    }

    pub(crate) fn on_entity_chunk_unload_finalized(&self, pos: ChunkPos) {
        self.entity_manager.finalize_chunk_unload(pos);
    }

    /// Spawns an item entity at the given position.
    ///
    /// This is a convenience method for dropping items in the world.
    ///
    /// Returns `None` if the item stack is empty.
    pub fn spawn_item(self: &Arc<Self>, pos: DVec3, item: ItemStack) -> Option<Arc<ItemEntity>> {
        // Default ItemEntity velocity: random horizontal scatter + upward pop
        let vx = rand::random::<f64>() * 0.2 - 0.1;
        let vy = 0.2;
        let vz = rand::random::<f64>() * 0.2 - 0.1;
        self.spawn_item_with_velocity(pos, item, DVec3::new(vx, vy, vz))
    }

    /// Spawns an item entity at the given position with initial velocity.
    ///
    /// Returns `None` if the item stack is empty.
    pub fn spawn_item_with_velocity(
        self: &Arc<Self>,
        pos: DVec3,
        item: ItemStack,
        velocity: DVec3,
    ) -> Option<Arc<ItemEntity>> {
        use crate::entity::next_entity_id;

        if item.is_empty() {
            return None;
        }

        let entity_id = next_entity_id();
        let entity = Arc::new(ItemEntity::with_item_and_velocity(
            &vanilla_entities::ITEM,
            entity_id,
            pos,
            item,
            velocity,
            Arc::downgrade(self),
        ));
        if let Err(error) = self.try_add_entity(entity.clone()) {
            log::warn!("Failed to spawn item entity: {error}");
            return None;
        }
        Some(entity)
    }

    /// Drops an item at a block position with random offset and velocity.
    ///
    /// Mirrors vanilla's `Block.popResource()`. Used for block drops.
    /// The item spawns near the center of the block with slight random offset
    /// and small random velocity.
    pub fn pop_resource(
        self: &Arc<Self>,
        pos: BlockPos,
        item: ItemStack,
    ) -> Option<Arc<ItemEntity>> {
        use steel_registry::vanilla_entities;

        if item.is_empty() {
            return None;
        }

        // Respect doTileDrops gamerule
        if !self.get_game_rule(&BLOCK_DROPS).as_bool().unwrap_or(true) {
            return None;
        }

        // Vanilla uses EntityType.ITEM dimensions for offset calculation
        let half_height = f64::from(vanilla_entities::ITEM.dimensions.height) / 2.0;

        // Random offset within block (vanilla: nextDouble(-0.25, 0.25))
        let x = f64::from(pos.x()) + 0.5 + (rand::random::<f64>() - 0.5) * 0.5;
        let y = f64::from(pos.y()) + 0.5 + (rand::random::<f64>() - 0.5) * 0.5 - half_height;
        let z = f64::from(pos.z()) + 0.5 + (rand::random::<f64>() - 0.5) * 0.5;

        let entity = self.spawn_item(DVec3::new(x, y, z), item)?;
        entity.set_default_pickup_delay();
        Some(entity)
    }

    /// Drops an item from a block face with directional velocity.
    ///
    /// Mirrors vanilla's `Block.popResourceFromFace()`. Used for items ejected
    /// from a specific side of a block.
    pub fn pop_resource_from_face(
        self: &Arc<Self>,
        pos: BlockPos,
        face: Direction,
        item: ItemStack,
    ) -> Option<Arc<ItemEntity>> {
        use steel_registry::vanilla_entities;

        if item.is_empty() {
            return None;
        }

        let half_width = f64::from(vanilla_entities::ITEM.dimensions.width) / 2.0;
        let half_height = f64::from(vanilla_entities::ITEM.dimensions.height) / 2.0;

        let (step_x, step_y, step_z) = face.offset();

        // Position calculation (vanilla logic)
        let x = f64::from(pos.x())
            + 0.5
            + if step_x == 0 {
                (rand::random::<f64>() - 0.5) * 0.5
            } else {
                f64::from(step_x) * (0.5 + half_width)
            };
        let y = f64::from(pos.y())
            + 0.5
            + if step_y == 0 {
                (rand::random::<f64>() - 0.5) * 0.5
            } else {
                f64::from(step_y) * (0.5 + half_height)
            }
            - half_height;
        let z = f64::from(pos.z())
            + 0.5
            + if step_z == 0 {
                (rand::random::<f64>() - 0.5) * 0.5
            } else {
                f64::from(step_z) * (0.5 + half_width)
            };

        // Velocity in direction of face
        let delta_x = if step_x == 0 {
            (rand::random::<f64>() - 0.5) * 0.2
        } else {
            f64::from(step_x) * 0.1
        };
        let delta_y = if step_y == 0 {
            rand::random::<f64>() * 0.1
        } else {
            f64::from(step_y) * 0.1 + 0.1
        };
        let delta_z = if step_z == 0 {
            (rand::random::<f64>() - 0.5) * 0.2
        } else {
            f64::from(step_z) * 0.1
        };

        let entity = self.spawn_item_with_velocity(
            DVec3::new(x, y, z),
            item,
            DVec3::new(delta_x, delta_y, delta_z),
        )?;
        entity.set_default_pickup_delay();
        Some(entity)
    }

    /// Gets an entity by its network ID.
    ///
    /// Returns `None` if the entity is not live in the world.
    #[must_use]
    pub fn get_entity_by_id(&self, id: i32) -> Option<SharedEntity> {
        self.entity_manager.get_by_id(id)
    }

    /// Gets an entity by its network ID if it is visible to vanilla gameplay lookups.
    ///
    /// Returns `None` if the entity is not live or is hidden in an inaccessible chunk.
    #[must_use]
    pub fn get_accessible_entity_by_id(&self, id: i32) -> Option<SharedEntity> {
        self.entity_manager.get_accessible_by_id(id)
    }

    /// Gets an entity by its UUID.
    ///
    /// Returns `None` if the entity is not live in the world.
    #[must_use]
    pub fn get_entity_by_uuid(&self, uuid: &uuid::Uuid) -> Option<SharedEntity> {
        self.entity_manager.get_by_uuid(uuid)
    }

    /// Gets all entities intersecting the given bounding box.
    ///
    /// Only returns entities in loaded chunks.
    #[must_use]
    pub fn get_entities_in_aabb(&self, aabb: &WorldAabb) -> Vec<SharedEntity> {
        self.entity_manager.get_entities_in_aabb(aabb)
    }

    /// Gets entities intersecting the given bounding box and matching `predicate`.
    ///
    /// Only returns entities in loaded chunks.
    #[must_use]
    pub fn get_entities_in_aabb_matching(
        &self,
        aabb: &WorldAabb,
        predicate: impl FnMut(&dyn Entity) -> bool,
    ) -> Vec<SharedEntity> {
        self.entity_manager
            .get_entities_in_aabb_matching(aabb, predicate)
    }

    /// Returns whether any entity intersects the given bounding box and matches `predicate`.
    ///
    /// Only checks entities in loaded chunks.
    #[must_use]
    pub fn has_entity_in_aabb_matching(
        &self,
        aabb: &WorldAabb,
        predicate: impl FnMut(&dyn Entity) -> bool,
    ) -> bool {
        self.entity_manager
            .has_entity_in_aabb_matching(aabb, predicate)
    }

    /// Gets matching entity bounding boxes intersecting the given bounding box.
    ///
    /// Only checks entities in loaded chunks.
    #[must_use]
    pub fn get_entity_bounding_boxes_in_aabb_matching(
        &self,
        aabb: &WorldAabb,
        predicate: impl FnMut(&dyn Entity) -> bool,
    ) -> Vec<WorldAabb> {
        self.entity_manager
            .get_entity_bounding_boxes_in_aabb_matching(aabb, predicate)
    }

    /// Gets the nearest entity intersecting the given bounding box and matching `predicate`.
    ///
    /// Only returns entities in loaded chunks.
    #[must_use]
    pub fn nearest_entity_in_aabb_matching(
        &self,
        aabb: &WorldAabb,
        origin: DVec3,
        predicate: impl FnMut(&dyn Entity) -> bool,
    ) -> Option<SharedEntity> {
        self.entity_manager
            .nearest_entity_in_aabb_matching(aabb, origin, predicate)
    }

    /// Gets the nearest player to `position` within `max_distance`.
    #[must_use]
    pub fn nearest_player(
        &self,
        position: DVec3,
        max_distance: f64,
        mut predicate: impl FnMut(&Player) -> bool,
    ) -> Option<Arc<Player>> {
        let max_distance_sqr = max_distance * max_distance;
        let mut nearest: Option<(Arc<Player>, f64)> = None;
        self.players.iter_players(|_, player| {
            if predicate(player) {
                let distance_sqr = player.position().distance_squared(position);
                if nearest_player_distance_in_range(distance_sqr, max_distance, max_distance_sqr)
                    && nearest
                        .as_ref()
                        .is_none_or(|(_, current)| distance_sqr < *current)
                {
                    nearest = Some((player.clone(), distance_sqr));
                }
            }
            true
        });
        nearest.map(|(player, _)| player)
    }

    /// Gets the squared distance to the nearest player, if any player is present.
    #[must_use]
    pub fn nearest_player_distance_sqr(&self, position: DVec3) -> Option<f64> {
        let mut nearest = None;
        self.players.iter_players(|_, player| {
            if player.is_spectator() {
                return true;
            }
            let distance_sqr = player.position().distance_squared(position);
            if nearest.is_none_or(|current| distance_sqr < current) {
                nearest = Some(distance_sqr);
            }
            true
        });
        nearest
    }

    /// Gets entities matching vanilla's pushable entity selector for `pusher`.
    ///
    /// Vanilla also checks team collision rules; Steel has no teams yet, so this
    /// currently matches the null-team path where collision is allowed.
    #[must_use]
    pub fn get_pushable_entities(
        &self,
        pusher: &dyn Entity,
        aabb: &WorldAabb,
    ) -> Vec<SharedEntity> {
        self.get_entities_in_aabb(aabb)
            .into_iter()
            .filter(|entity| entity.id() != pusher.id())
            .filter(|entity| !entity.is_spectator())
            .filter(|entity| entity.is_pushable())
            .collect()
    }

    /// Registers a game event listener in a chunk section.
    pub fn register_game_event_listener(
        &self,
        section_pos: SectionPos,
        listener: SharedGameEventListener,
    ) {
        self.game_event_listeners.register(section_pos, listener);
    }

    /// Unregisters a game event listener from a chunk section.
    pub fn unregister_game_event_listener(
        &self,
        section_pos: SectionPos,
        listener: &SharedGameEventListener,
    ) -> bool {
        self.game_event_listeners.unregister(section_pos, listener)
    }

    /// Dispatches a game event to all listeners in range.
    pub fn game_event(
        self: &Arc<Self>,
        event: GameEventRef,
        pos: BlockPos,
        context: &GameEventContext,
    ) {
        self.game_event_at(
            event,
            DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 0.5,
                f64::from(pos.z()) + 0.5,
            ),
            context,
        );
    }

    /// Dispatches a game event from an exact world position.
    pub fn game_event_at(
        self: &Arc<Self>,
        event: GameEventRef,
        source_pos: DVec3,
        context: &GameEventContext,
    ) {
        self.game_event_listeners
            .dispatch(self, event, source_pos, context);
    }
}

fn nearest_player_distance_in_range(
    distance_sqr: f64,
    max_distance: f64,
    max_distance_sqr: f64,
) -> bool {
    max_distance < 0.0 || distance_sqr < max_distance_sqr
}

impl LevelReader for World {
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        Self::get_block_state(self, pos)
    }

    fn get_block_entity(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        Self::get_block_entity(self, pos)
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

    fn schedule_fluid_tick_default(&self, pos: BlockPos, fluid: FluidRef, delay: i32) -> bool {
        self.as_ref().schedule_fluid_tick_default(pos, fluid, delay);
        true
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Weak};

    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::{test_support::init_test_registry, vanilla_entities, vanilla_fluids};
    use uuid::Uuid;

    use crate::behavior::init_behaviors;
    use crate::entity::{EntityBase, entities::PigEntity};

    const FIRST_HALF: BlockLocalAabb = BlockLocalAabb::new(0.0, 0.0, 0.0, 0.5, 1.0, 1.0);
    const SECOND_HALF: BlockLocalAabb = BlockLocalAabb::new(0.5, 0.0, 0.0, 1.0, 1.0, 1.0);
    static SPLIT_BLOCK: &[BlockLocalAabb] = &[FIRST_HALF, SECOND_HALF];

    struct TrackerTestEntity {
        base: EntityBase,
    }

    impl TrackerTestEntity {
        fn shared(id: i32) -> SharedEntity {
            Arc::new(Self {
                base: EntityBase::with_uuid(
                    id,
                    Uuid::from_u128(id as u128),
                    DVec3::ZERO,
                    vanilla_entities::ITEM.dimensions,
                    Weak::new(),
                ),
            })
        }
    }

    impl Entity for TrackerTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::ITEM
        }
    }

    fn assert_vec3_close(left: DVec3, right: DVec3) {
        let diff = left - right;
        assert!(
            diff.length_squared() < 1.0e-24,
            "expected {left:?} to equal {right:?}"
        );
    }

    #[test]
    fn nearest_player_range_uses_vanilla_strict_boundary() {
        assert!(nearest_player_distance_in_range(63.999, 8.0, 64.0));
        assert!(!nearest_player_distance_in_range(64.0, 8.0, 64.0));
    }

    #[test]
    fn nearest_player_negative_range_is_unbounded() {
        assert!(nearest_player_distance_in_range(1_000_000.0, -1.0, 1.0));
    }

    #[test]
    fn spawnable_bounds_match_vanilla_teleport_command_bounds() {
        assert!(World::is_in_spawnable_bounds(BlockPos::new(0, 320, 0)));
        assert!(World::is_in_spawnable_bounds(BlockPos::new(
            29_999_999,
            19_999_999,
            -30_000_000
        )));
        assert!(!World::is_in_spawnable_bounds(BlockPos::new(
            30_000_000, 0, 0
        )));
        assert!(!World::is_in_spawnable_bounds(BlockPos::new(
            0,
            -20_000_001,
            0
        )));
        assert!(!World::is_in_spawnable_bounds(BlockPos::new(
            0, 20_000_000, 0
        )));
    }

    #[test]
    fn light_packet_tracking_border_matches_vanilla_pending_chunk_rule() {
        let view = PlayerChunkView::new(ChunkPos::new(0, 0), 2);
        let center = ChunkPos::new(0, 0);

        assert!(!World::chunk_is_on_packet_tracked_border(
            view,
            center,
            &|pos| view.contains(pos)
        ));
        assert!(World::chunk_is_on_packet_tracked_border(
            view,
            ChunkPos::new(3, 0),
            &|_| true
        ));
        assert!(World::chunk_is_on_packet_tracked_border(
            view,
            center,
            &|pos| pos != ChunkPos::new(1, 0)
        ));
        assert!(!World::chunk_is_on_packet_tracked_border(
            view,
            center,
            &|pos| pos != center
        ));
    }

    #[test]
    fn navigating_mob_tracker_tracks_only_pathfinder_mobs() {
        init_test_registry();

        let tracker = NavigatingMobTracker::new();
        let non_pathfinder = TrackerTestEntity::shared(1);
        let pig: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::ZERO,
            Weak::new(),
        ));

        tracker.track(&non_pathfinder);
        assert!(tracker.ids().is_empty());

        tracker.track(&pig);
        tracker.track(&pig);
        assert_eq!(tracker.ids(), [2]);

        tracker.untrack(2);
        assert!(tracker.ids().is_empty());
    }

    #[test]
    fn clip_shape_hits_closest_block_face() {
        let Some(hit) = World::clip_shape(
            BlockPos::ZERO,
            DVec3::new(-1.0, 0.5, 0.5),
            DVec3::new(1.0, 0.5, 0.5),
            OffsetVoxelShape::without_offset(VoxelShape::from_boxes(SPLIT_BLOCK)),
        ) else {
            panic!("expected shape hit");
        };

        assert!(!hit.inside);
        assert_eq!(hit.direction, Direction::West);
        assert_eq!(hit.block_pos, BlockPos::ZERO);
        assert_vec3_close(hit.location, DVec3::new(0.0, 0.5, 0.5));
    }

    #[test]
    fn clip_shape_reports_inside_start_like_vanilla_voxel_shape() {
        let Some(hit) = World::clip_shape(
            BlockPos::ZERO,
            DVec3::new(0.5, 0.5, 0.5),
            DVec3::new(2.5, 0.5, 0.5),
            OffsetVoxelShape::without_offset(VoxelShape::FULL_BLOCK),
        ) else {
            panic!("expected inside shape hit");
        };

        assert!(hit.inside);
        assert_eq!(hit.direction, Direction::West);
        assert_vec3_close(hit.location, DVec3::new(0.502, 0.5, 0.5));
    }

    #[test]
    fn clip_local_aabb_supports_runtime_fluid_heights() {
        let Some(hit) = World::clip_local_aabb(
            BlockPos::ZERO,
            DVec3::new(0.5, 2.0, 0.5),
            DVec3::new(0.5, 0.0, 0.5),
            BlockLocalAabb::new(0.0, 0.0, 0.0, 1.0, 0.5, 1.0),
        ) else {
            panic!("expected fluid shape hit");
        };

        assert_eq!(hit.direction, Direction::Up);
        assert_vec3_close(hit.location, DVec3::new(0.5, 0.5, 0.5));
    }

    #[test]
    fn fluid_clip_height_treats_source_and_flowing_variants_as_same_fluid_above() {
        init_test_registry();
        init_behaviors();

        let height = World::fluid_clip_height_from_above(
            FluidState::source(&vanilla_fluids::WATER),
            FluidState::flowing(&vanilla_fluids::FLOWING_WATER, 4, false),
        );

        assert_eq!(height.to_bits(), 1.0_f64.to_bits());
    }
}
