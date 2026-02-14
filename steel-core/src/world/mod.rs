//! This module contains the `World` struct, which represents a world.

use std::path::Path;
use std::{
    io,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    time::Duration,
};

use crate::{chunk::chunk_map::ChunkMapTickTimings, world::weather::Weather};

use sha2::{Digest, Sha256};
use steel_protocol::packets::game::{
    CBlockDestruction, CBlockEvent, CGameEvent, CLevelEvent, CPlayerChat, CPlayerInfoUpdate,
    CRemoveEntities, CSound, CSystemChat, GameEventType, SoundSource,
};
use steel_protocol::utils::ConnectionProtocol;
use steel_protocol::{
    packet_traits::{ClientPacket, EncodedPacket},
    packets::game::CSetTime,
};

use simdnbt::owned::NbtCompound;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::fluid::FluidRef;
use steel_registry::game_rules::{GameRuleRef, GameRuleValue};
use steel_registry::item_stack::ItemStack;
use steel_registry::level_events;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_game_rules::RANDOM_TICK_SPEED;
use steel_registry::{REGISTRY, dimension_type::DimensionTypeRef};
use steel_registry::{block_entity_type::BlockEntityTypeRef, vanilla_dimension_types};
use steel_registry::{
    blocks::BlockRef, vanilla_game_rules::ADVANCE_TIME, vanilla_game_rules::ADVANCE_WEATHER,
};

use steel_registry::blocks::shapes::{AABBd, VoxelShape};
use steel_utils::locks::{SyncMutex, SyncRwLock};
use steel_utils::math::Vector3;
use steel_utils::{BlockPos, BlockStateId, ChunkPos, SectionPos, types::UpdateFlags};
use tokio::{runtime::Runtime, time::Instant};

use crate::{
    ChunkMap,
    behavior::BLOCK_BEHAVIORS,
    block_entity::SharedBlockEntity,
    chunk_saver::{ChunkStorage, RamOnlyStorage, RegionManager},
    config::STEEL_CONFIG,
    entity::{EntityCache, EntityTracker, RemovalReason, SharedEntity, entities::ItemEntity},
    level_data::LevelDataManager,
    player::{LastSeen, Player, connection::NetworkConnection},
};

mod player_area_map;
mod player_map;
pub mod tick_scheduler;
mod weather;
mod world_entities;

use crate::chunk::world_gen_context::ChunkGeneratorType;
pub use crate::config::WorldStorageConfig;
pub use player_area_map::PlayerAreaMap;
pub use player_map::PlayerMap;

/// Generates a random value using triangle distribution.
///
/// Mirrors vanilla's `RandomSource.triangle(mode, deviation)`.
/// Produces values centered around `mode` with a spread of `deviation`.
fn triangle_random(mode: f64, deviation: f64) -> f64 {
    mode + deviation * (rand::random::<f64>() - rand::random::<f64>())
}

/// Timing information for a world tick.
#[derive(Debug)]
pub struct WorldTickTimings {
    /// Chunk map tick timings.
    pub chunk_map: ChunkMapTickTimings,
    /// Time spent ticking players.
    pub player_tick: Duration,
}

/// Interval in ticks between player info broadcasts (600 ticks = 30 seconds).
/// Matches vanilla `PlayerList.SEND_PLAYER_INFO_INTERVAL`.
const SEND_PLAYER_INFO_INTERVAL: u64 = 600;

/// Configuration for creating a new world.
#[derive(Clone)]
pub struct WorldConfig {
    /// Storage configuration for chunk persistence.
    pub storage: WorldStorageConfig,
    /// World generator.
    pub generator: Arc<ChunkGeneratorType>,
}

/// A struct that represents a world.
pub struct World {
    /// The chunk map of the world.
    pub chunk_map: Arc<ChunkMap>,
    /// All players in the world with dual indexing by UUID and entity ID.
    pub players: PlayerMap,
    /// Spatial index for player proximity queries.
    pub player_area_map: PlayerAreaMap,
    /// The dimension of the world.
    pub dimension: DimensionTypeRef,
    /// Level data manager for persistent world state.
    pub level_data: SyncRwLock<LevelDataManager>,
    /// Whether the tick rate is running normally (not frozen/paused).
    /// When false, movement validation checks are skipped.
    tick_runs_normally: AtomicBool,
    /// Entity cache for fast entity lookups by ID, UUID, or spatial position.
    /// Uses `Weak` references - entities are owned by chunks.
    entity_cache: EntityCache,
    /// Entity tracker for managing which players can see which entities.
    entity_tracker: EntityTracker,
    /// Weather Data needed for animating starting and stopping of rain clientside
    pub weather: SyncMutex<Weather>,
    /// Monotonic counter for `sub_tick_order` on scheduled ticks.
    /// Provides stable ordering when multiple ticks fire on the same game tick
    /// with the same priority.
    sub_tick_count: AtomicI64,
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
    /// * `dimension` - The dimension type (overworld, nether, end)
    /// * `seed` - The world seed
    /// * `config` - World configuration including storage options
    pub async fn new_with_config(
        chunk_runtime: Arc<Runtime>,
        dimension: DimensionTypeRef,
        seed: i64,
        config: WorldConfig,
    ) -> io::Result<Arc<Self>> {
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

        let path = match &config.storage {
            WorldStorageConfig::Disk { path } => Some(Path::new(path)),
            WorldStorageConfig::RamOnly => None,
        };
        let level_data = LevelDataManager::new(path, seed).await?;
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

        Ok(Arc::new_cyclic(|weak_self: &Weak<World>| Self {
            chunk_map: Arc::new(ChunkMap::new_with_storage(
                chunk_runtime,
                weak_self.clone(),
                &dimension,
                storage,
                config.generator,
            )),
            players: PlayerMap::new(),
            player_area_map: PlayerAreaMap::new(),
            dimension,
            level_data: SyncRwLock::new(level_data),
            tick_runs_normally: AtomicBool::new(true),
            entity_cache: EntityCache::new(),
            entity_tracker: EntityTracker::new(),
            weather: SyncMutex::new(weather),
            sub_tick_count: AtomicI64::new(0),
        }))
    }

    /// Cleans up the world by saving all chunks.
    /// `await_holding_lock` is safe here cause it's only done on shutdown
    #[allow(clippy::await_holding_lock)]
    pub async fn cleanup(&self, total_saved: &mut usize) {
        match self.level_data.write().save().await {
            Ok(()) => log::info!(
                "World {} level data saved successfully",
                self.dimension.key.path
            ),
            Err(e) => log::error!("Failed to save world level data: {e}"),
        }

        match self.save_all_chunks().await {
            Ok(count) => *total_saved += count,
            Err(e) => log::error!("Failed to save world chunks: {e}"),
        }
    }

    /// Returns the total height of the world in blocks.
    pub const fn get_height(&self) -> i32 {
        self.dimension.height
    }

    /// Returns the minimum Y coordinate of the world.
    pub const fn get_min_y(&self) -> i32 {
        self.dimension.min_y
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
    pub const fn is_in_valid_bounds_horizontal(&self, block_pos: &BlockPos) -> bool {
        let chunk_x = SectionPos::block_to_section_coord(block_pos.0.x);
        let chunk_z = SectionPos::block_to_section_coord(block_pos.0.z);
        ChunkPos::is_valid(chunk_x, chunk_z)
    }

    /// Returns whether the block position is within valid world bounds.
    pub const fn is_in_valid_bounds(&self, block_pos: &BlockPos) -> bool {
        !self.is_outside_build_height(block_pos.0.y)
            && self.is_in_valid_bounds_horizontal(block_pos)
    }

    /// Returns the maximum build height (one above the highest placeable block).
    /// This is `min_y + height`.
    #[must_use]
    pub const fn max_build_height(&self) -> i32 {
        self.get_min_y() + self.get_height()
    }

    /// Checks if a player may interact with the world at the given position.
    /// Currently only checks if position is within world bounds.
    #[must_use]
    pub const fn may_interact(&self, _player: &Player, pos: &BlockPos) -> bool {
        self.is_in_valid_bounds(pos)
    }

    /// Player dimensions matching vanilla Minecraft.
    const PLAYER_WIDTH: f64 = 0.6;
    const PLAYER_HEIGHT: f64 = 1.8;

    /// Checks if a block's collision shape at the given position is unobstructed by entities.
    ///
    /// This is the Rust equivalent of vanilla's `Level.isUnobstructed(BlockState, BlockPos, CollisionContext)`.
    /// In vanilla, this checks all entities with `blocksBuilding=true` (players, mobs, boats, etc.).
    /// Currently only checks players since other entities aren't fully implemented.
    ///
    /// Returns `true` if the position is clear, `false` if an entity would obstruct placement.
    #[must_use]
    pub fn is_unobstructed(&self, collision_shape: VoxelShape, pos: &BlockPos) -> bool {
        if collision_shape.is_empty() {
            return true;
        }

        // TODO: Check other entities with blocksBuilding=true (mobs, boats, minecarts, etc.)
        let mut obstructed = false;
        self.players.iter_players(|_uuid, player| {
            let player_pos = player.position.lock();
            let half_width = Self::PLAYER_WIDTH / 2.0;
            let player_aabb = AABBd::new(
                player_pos.x - half_width,
                player_pos.y,
                player_pos.z - half_width,
                player_pos.x + half_width,
                player_pos.y + Self::PLAYER_HEIGHT,
                player_pos.z + half_width,
            );

            // Check if any block AABB intersects with the player
            for block_aabb in collision_shape {
                let world_aabb = block_aabb.at_block(pos.x(), pos.y(), pos.z());
                if player_aabb.intersects_block_aabb(&world_aabb) {
                    obstructed = true;
                    return false; // stop iteration
                }
            }

            true // continue iteration
        });

        !obstructed
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
    #[allow(clippy::missing_panics_doc)] // SHA-256 always produces 32 bytes
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
    pub fn get_block_state(&self, pos: &BlockPos) -> BlockStateId {
        if !self.is_in_valid_bounds(pos) {
            return REGISTRY.blocks.get_base_state_id(vanilla_blocks::AIR);
        }

        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map
            .with_full_chunk(&chunk_pos, |chunk| chunk.get_block_state(*pos))
            .unwrap_or_else(|| REGISTRY.blocks.get_base_state_id(vanilla_blocks::AIR))
    }

    /// Sets a block at the given position.
    ///
    /// Returns `true` if the block was successfully set, `false` otherwise.
    /// Uses the default update limit of 512 (matching vanilla).
    pub fn set_block(&self, pos: BlockPos, block_state: BlockStateId, flags: UpdateFlags) -> bool {
        self.set_block_with_limit(pos, block_state, flags, 512)
    }

    /// Sets a block at the given position with a custom update limit.
    ///
    /// The update limit prevents infinite recursion when shape updates trigger
    /// further block changes. Each recursive call decrements the limit.
    ///
    /// Returns `true` if the block was successfully set, `false` otherwise.
    pub fn set_block_with_limit(
        &self,
        pos: BlockPos,
        block_state: BlockStateId,
        flags: UpdateFlags,
        update_limit: i32,
    ) -> bool {
        if update_limit <= 0 {
            return false;
        }

        if !self.is_in_valid_bounds(&pos) {
            return false;
        }

        let chunk_pos = Self::chunk_pos_for_block(&pos);
        let Some(old_state) = self
            .chunk_map
            .with_full_chunk(&chunk_pos, |chunk| {
                chunk.set_block_state(pos, block_state, flags)
            })
            .flatten()
        else {
            return false;
        };

        // Record the block change for broadcasting to clients
        log::debug!("Block changed at {pos:?}: {old_state:?} -> {block_state:?}");
        self.chunk_map.block_changed(&pos);

        // Neighbor updates (when UPDATE_NEIGHBORS is set)
        if flags.contains(UpdateFlags::UPDATE_NEIGHBORS) {
            self.update_neighbors_at(&pos, old_state.get_block());
            // TODO: if block has analog output signal, update comparator neighbors
            // via updateNeighbourForOutputSignal
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
    pub fn update_neighbors_at(&self, pos: &BlockPos, source_block: BlockRef) {
        for direction in Self::NEIGHBOR_UPDATE_ORDER {
            let neighbor_pos = pos.relative(direction);
            self.neighbor_changed(neighbor_pos, source_block, false);
        }
    }

    /// Called when a neighbor's shape changes, to update this block's state.
    ///
    /// This is the Rust equivalent of vanilla's `NeighborUpdater.executeShapeUpdate()`.
    fn neighbor_shape_changed(
        &self,
        direction: Direction,
        pos: BlockPos,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
        flags: UpdateFlags,
        update_limit: i32,
    ) {
        if !self.is_in_valid_bounds(&pos) {
            return;
        }

        let current_state = self.get_block_state(&pos);

        // TODO: Skip redstone wire if UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE is set
        // if flags.contains(UpdateFlags::UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE)
        //     && current_state.is_redstone_wire() { return; }

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

        if new_state != current_state {
            log::debug!(
                "Shape update at {pos:?}: {current_state:?} -> {new_state:?} (neighbor {neighbor_pos:?} changed)"
            );
            // Use set_block_with_limit to prevent infinite recursion
            self.set_block_with_limit(pos, new_state, flags, update_limit);
        }
    }

    /// Notifies a block that one of its neighbors changed.
    ///
    /// This is the Rust equivalent of vanilla's `Level.neighborChanged()`.
    fn neighbor_changed(&self, pos: BlockPos, source_block: BlockRef, moved_by_piston: bool) {
        if !self.is_in_valid_bounds(&pos) {
            return;
        }

        let state = self.get_block_state(&pos);
        let block_behaviors = &*BLOCK_BEHAVIORS;
        let behavior = block_behaviors.get_behavior(state.get_block());
        behavior.handle_neighbor_changed(state, self, pos, source_block, moved_by_piston);
    }

    const fn chunk_pos_for_block(pos: &BlockPos) -> ChunkPos {
        ChunkPos::new(
            SectionPos::block_to_section_coord(pos.0.x),
            SectionPos::block_to_section_coord(pos.0.z),
        )
    }

    /// Gets a block entity at the given position.
    ///
    /// Returns `None` if the chunk is not loaded or there is no block entity at the position.
    #[must_use]
    pub fn get_block_entity(&self, pos: &BlockPos) -> Option<SharedBlockEntity> {
        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map
            .with_full_chunk(&chunk_pos, |chunk| {
                chunk.as_full().and_then(|lc| lc.get_block_entity(*pos))
            })
            .flatten()
    }

    /// Called when a block entity's data changes.
    ///
    /// Marks the containing chunk as unsaved so it will be persisted to disk.
    pub fn block_entity_changed(&self, pos: BlockPos) {
        let chunk_pos = Self::chunk_pos_for_block(&pos);
        self.mark_chunk_dirty(chunk_pos);
    }

    /// Marks a chunk as dirty (unsaved) so it will be persisted to disk.
    ///
    /// Called when entities move, are added/removed, or when block entities change.
    pub fn mark_chunk_dirty(&self, chunk_pos: ChunkPos) {
        self.chunk_map.with_full_chunk(&chunk_pos, |chunk| {
            if let Some(lc) = chunk.as_full() {
                lc.dirty.store(true, Ordering::Release);
            }
        });
    }

    /// Ticks the world.
    ///
    /// * `tick_count` - The current tick number
    /// * `runs_normally` - Whether game elements (random ticks, entities) should run.
    ///   When false (frozen), only essential operations like chunk loading run.
    ///
    /// Returns timing information for the world tick.
    #[tracing::instrument(level = "trace", skip(self), name = "world_tick")]
    pub fn tick_b(&self, tick_count: u64, runs_normally: bool) -> WorldTickTimings {
        if runs_normally {
            self.tick_weather();
            self.tick_time();
        }

        let random_tick_speed = self.get_game_rule(RANDOM_TICK_SPEED).as_int().unwrap_or(3) as u32;

        let chunk_map_timings =
            self.chunk_map
                .tick_b(self, tick_count, random_tick_speed, runs_normally);

        // Tick players (always tick players - they can move when frozen)
        let player_tick = {
            let _span = tracing::trace_span!("player_tick").entered();
            let start = Instant::now();
            self.players.iter_players(|_uuid, player| {
                player.tick();
                true
            });
            start.elapsed()
        };

        // Broadcast player latency updates periodically
        if tick_count.is_multiple_of(SEND_PLAYER_INFO_INTERVAL) {
            let _span = tracing::trace_span!("broadcast_latency").entered();
            self.broadcast_player_latency_updates();
        }

        WorldTickTimings {
            chunk_map: chunk_map_timings,
            player_tick,
        }
    }

    #[expect(clippy::too_many_lines)]
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
                .get_game_rule_with_guard(ADVANCE_WEATHER, &level_data)
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
            #[expect(clippy::float_cmp)]
            if weather.previous_rain_level != weather.rain_level {
                self.broadcast_to_all(CGameEvent {
                    event: GameEventType::RainLevelChange,
                    data: weather.rain_level,
                });
            }

            #[expect(clippy::float_cmp)]
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

    /// Checks whether the world can have weather.
    pub fn can_have_weather(&self) -> bool {
        self.dimension.has_skylight
            && !self.dimension.has_ceiling
            && self.dimension.key != vanilla_dimension_types::THE_END.key
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
        let chunk_pos = Self::chunk_pos_for_block(&pos);
        self.chunk_map.with_full_chunk(&chunk_pos, |chunk_access| {
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
        let chunk_pos = Self::chunk_pos_for_block(&pos);
        self.chunk_map.with_full_chunk(&chunk_pos, |chunk_access| {
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
        let chunk_pos = Self::chunk_pos_for_block(&pos);
        self.chunk_map
            .with_full_chunk(&chunk_pos, |chunk_access| {
                chunk_access
                    .as_full()
                    .is_some_and(|chunk| chunk.block_ticks.lock().has_tick(pos, block))
            })
            .unwrap_or(false)
    }

    /// Returns `true` if a fluid tick is already scheduled for the given `(pos, fluid)`.
    pub fn has_scheduled_fluid_tick(&self, pos: BlockPos, fluid: FluidRef) -> bool {
        let chunk_pos = Self::chunk_pos_for_block(&pos);
        self.chunk_map
            .with_full_chunk(&chunk_pos, |chunk_access| {
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
            .get_game_rule(ADVANCE_TIME)
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
            self.broadcast_to_all(CSetTime {
                game_time,
                day_time,
                time_of_day_increasing: advance_time,
            });
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
        message_signature: Option<[u8; 256]>,
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
                let recipient_cache = recipient.signature_cache.lock();
                recipient_cache.index_previous_messages(&sender_last_seen)
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
            if let Some(signature) = message_signature {
                recipient
                    .signature_cache
                    .lock()
                    .push(&sender_last_seen, Some(&signature));

                log::debug!("  Added signature to recipient's cache and pending list");

                // Add to pending messages for acknowledgment tracking
                recipient
                    .message_validator
                    .lock()
                    .add_pending(Some(Box::new(signature) as Box<[u8]>));
            } else {
                // Even unsigned messages update the pending tracker
                recipient.message_validator.lock().add_pending(None);
                log::debug!("  Added unsigned message to pending list");
            }

            true
        });
    }

    /// Broadcasts a system chat message to all players.
    pub fn broadcast_system_chat(&self, packet: CSystemChat) {
        self.broadcast_to_all(packet);
    }

    /// Broadcasts a packet to all players in the world.
    ///
    /// This method handles encoding the packet once and sending it to all players,
    /// avoiding repeated cloning of unencoded packets.
    pub fn broadcast_to_all<P: ClientPacket>(&self, packet: P) {
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, STEEL_CONFIG.compression, ConnectionProtocol::Play)
        else {
            return;
        };
        self.broadcast_to_all_encoded(encoded);
    }

    /// Broadcasts a packet to all players in the world.
    ///
    /// This method handles encoding the packets producced from the function passed
    pub fn broadcast_to_all_with<P: ClientPacket, F: Fn(&Player) -> P>(&self, packet: F) {
        self.players.iter_players(|_, player| {
            let Ok(encoded) = EncodedPacket::from_bare(
                packet(player),
                STEEL_CONFIG.compression,
                ConnectionProtocol::Play,
            ) else {
                return false;
            };
            player.connection.send_encoded(encoded);
            true
        });
    }

    /// Broadcasts an already-encoded packet to all players in the world.
    ///
    /// Use this when you have a pre-encoded packet to avoid re-encoding.
    pub fn broadcast_to_all_encoded(&self, packet: EncodedPacket) {
        self.players.iter_players(|_, player| {
            player.connection.send_encoded(packet.clone());
            true
        });
    }

    /// Broadcasts an unsigned player chat message to all players.
    pub fn broadcast_unsigned_chat(
        &self,
        mut packet: CPlayerChat,
        sender_name: &str,
        message: &str,
    ) {
        log::info!("<{sender_name}> {message}");

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
            EncodedPacket::from_bare(packet, STEEL_CONFIG.compression, ConnectionProtocol::Play)
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
        let tracking_players = self.player_area_map.get_tracking_players(chunk);
        for entity_id in tracking_players {
            if Some(entity_id) == exclude {
                continue;
            }
            if let Some(player) = self.players.get_by_entity_id(entity_id) {
                player.connection.send_encoded(packet.clone());
            }
        }
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
    #[allow(clippy::cast_sign_loss)]
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
        let type_id = *REGISTRY.block_entity_types.get_id(block_entity_type);

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
                entity_id,
                Vector3::new(x, y, z),
                split_stack,
                Vector3::new(vx, vy, vz),
                Arc::downgrade(self),
            ));
            entity.set_default_pickup_delay();
            self.add_entity(entity);
        }
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
            EncodedPacket::from_bare(packet, STEEL_CONFIG.compression, ConnectionProtocol::Play)
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
                let player_pos = *player.position.lock();
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

        let block_id = *REGISTRY.blocks.get_id(block) as i32;

        let chunk = ChunkPos::new(
            SectionPos::block_to_section_coord(pos.x()),
            SectionPos::block_to_section_coord(pos.z()),
        );
        let packet = CBlockEvent::new(pos, action_id, action_param, block_id);
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, STEEL_CONFIG.compression, ConnectionProtocol::Play)
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
                let player_pos = *player.position.lock();
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
    /// * `sound_id` - The sound event registry ID (from `steel_registry::sound_events`)
    /// * `source` - The sound source category
    /// * `pos` - The block position (sound plays at center of block)
    /// * `volume` - Volume multiplier (1.0 = normal)
    /// * `pitch` - Pitch multiplier (1.0 = normal)
    /// * `exclude` - Optional entity ID to exclude from receiving the sound
    pub fn play_sound(
        &self,
        sound_id: i32,
        source: SoundSource,
        pos: BlockPos,
        volume: f32,
        pitch: f32,
        exclude: Option<i32>,
    ) {
        const MAX_DISTANCE_SQ: f64 = 64.0 * 64.0;

        let chunk = ChunkPos::new(
            SectionPos::block_to_section_coord(pos.x()),
            SectionPos::block_to_section_coord(pos.z()),
        );

        // Generate a random seed for sound variations
        let seed = rand::random::<i64>();

        let packet = CSound::new(
            sound_id,
            source,
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 0.5,
            f64::from(pos.z()) + 0.5,
            volume,
            pitch,
            seed,
        );
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, STEEL_CONFIG.compression, ConnectionProtocol::Play)
        else {
            log::warn!("Failed to encode sound packet");
            return;
        };

        // Get players tracking this chunk, then filter by 64-block distance
        let sound_pos = (
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 0.5,
            f64::from(pos.z()) + 0.5,
        );

        for entity_id in self.player_area_map.get_tracking_players(chunk) {
            // Skip excluded player (they hear the sound client-side)
            if exclude == Some(entity_id) {
                continue;
            }
            if let Some(player) = self.players.get_by_entity_id(entity_id) {
                let player_pos = *player.position.lock();
                let dx = player_pos.x - sound_pos.0;
                let dy = player_pos.y - sound_pos.1;
                let dz = player_pos.z - sound_pos.2;
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
    /// * `sound_id` - The sound event registry ID
    /// * `pos` - The block position
    /// * `volume` - Base volume (typically from `SoundType`)
    /// * `pitch` - Base pitch (typically from `SoundType`)
    /// * `exclude` - Optional entity ID to exclude from receiving the sound
    pub fn play_block_sound(
        &self,
        sound_id: i32,
        pos: BlockPos,
        volume: f32,
        pitch: f32,
        exclude: Option<i32>,
    ) {
        self.play_sound(sound_id, SoundSource::Blocks, pos, volume, pitch, exclude);
    }

    // === Entity Methods ===

    /// Returns a reference to the entity cache.
    #[must_use]
    pub const fn entity_cache(&self) -> &EntityCache {
        &self.entity_cache
    }

    /// Returns the entity tracker for managing player-entity visibility.
    #[must_use]
    pub const fn entity_tracker(&self) -> &EntityTracker {
        &self.entity_tracker
    }

    /// Adds an entity to the world.
    ///
    /// This delegates to the chunk's `add_and_register_entity` method which handles:
    /// - Adding to chunk storage
    /// - Setting up level callback
    /// - Registering in entity cache
    /// - Adding to entity tracker and sending spawn packets
    /// - Marking the chunk dirty
    pub fn add_entity(self: &Arc<Self>, entity: SharedEntity) {
        let pos = entity.position();
        let chunk_pos = ChunkPos::new((pos.x as i32) >> 4, (pos.z as i32) >> 4);

        self.chunk_map.with_full_chunk(&chunk_pos, |chunk| {
            if let Some(c) = chunk.as_full() {
                c.add_and_register_entity(entity.clone());
            }
        });
    }

    /// Spawns an item entity at the given position.
    ///
    /// This is a convenience method for dropping items in the world.
    /// The item will have a default pickup delay.
    ///
    /// Returns `None` if the item stack is empty.
    pub fn spawn_item(
        self: &Arc<Self>,
        pos: Vector3<f64>,
        item: ItemStack,
    ) -> Option<Arc<ItemEntity>> {
        self.spawn_item_with_velocity(pos, item, Vector3::new(0.0, 0.0, 0.0))
    }

    /// Spawns an item entity at the given position with initial velocity.
    ///
    /// Returns `None` if the item stack is empty.
    pub fn spawn_item_with_velocity(
        self: &Arc<Self>,
        pos: Vector3<f64>,
        item: ItemStack,
        velocity: Vector3<f64>,
    ) -> Option<Arc<ItemEntity>> {
        use crate::entity::next_entity_id;

        if item.is_empty() {
            return None;
        }

        let entity_id = next_entity_id();
        let entity = Arc::new(ItemEntity::with_item_and_velocity(
            entity_id,
            pos,
            item,
            velocity,
            Arc::downgrade(self),
        ));
        entity.set_default_pickup_delay();

        self.add_entity(entity.clone());
        Some(entity)
    }

    /// Drops an item at a block position with random offset and velocity.
    ///
    /// Mirrors vanilla's `Block.popResource()`. Used for block drops.
    /// The item spawns near the center of the block with slight random offset
    /// and small random velocity.
    pub fn pop_resource(
        self: &Arc<Self>,
        pos: &BlockPos,
        item: ItemStack,
    ) -> Option<Arc<ItemEntity>> {
        use steel_registry::vanilla_entities;

        if item.is_empty() {
            return None;
        }

        // Vanilla uses EntityType.ITEM dimensions for offset calculation
        let half_height = f64::from(vanilla_entities::ITEM.dimensions.height) / 2.0;

        // Random offset within block (vanilla: nextDouble(-0.25, 0.25))
        let x = f64::from(pos.x()) + 0.5 + (rand::random::<f64>() - 0.5) * 0.5;
        let y = f64::from(pos.y()) + 0.5 + (rand::random::<f64>() - 0.5) * 0.5 - half_height;
        let z = f64::from(pos.z()) + 0.5 + (rand::random::<f64>() - 0.5) * 0.5;

        self.spawn_item(Vector3::new(x, y, z), item)
    }

    /// Drops an item from a block face with directional velocity.
    ///
    /// Mirrors vanilla's `Block.popResourceFromFace()`. Used for items ejected
    /// from a specific side of a block.
    pub fn pop_resource_from_face(
        self: &Arc<Self>,
        pos: &BlockPos,
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

        self.spawn_item_with_velocity(
            Vector3::new(x, y, z),
            item,
            Vector3::new(delta_x, delta_y, delta_z),
        )
    }

    /// Gets an entity by its network ID.
    ///
    /// Returns `None` if the entity doesn't exist or its chunk was unloaded.
    #[must_use]
    pub fn get_entity_by_id(&self, id: i32) -> Option<SharedEntity> {
        self.entity_cache.get_by_id(id)
    }

    /// Gets an entity by its UUID.
    ///
    /// Returns `None` if the entity doesn't exist or its chunk was unloaded.
    #[must_use]
    pub fn get_entity_by_uuid(&self, uuid: &uuid::Uuid) -> Option<SharedEntity> {
        self.entity_cache.get_by_uuid(uuid)
    }

    /// Gets all entities intersecting the given bounding box.
    ///
    /// Only returns entities in loaded chunks.
    #[must_use]
    pub fn get_entities_in_aabb(&self, aabb: &AABBd) -> Vec<SharedEntity> {
        self.entity_cache.get_entities_in_aabb(aabb)
    }

    /// Moves an entity's Arc between chunks when it crosses a chunk boundary.
    ///
    /// Called by `EntityChunkCallback` when an entity moves between chunks.
    pub fn move_entity_between_chunks(&self, entity_id: i32, from: ChunkPos, to: ChunkPos) {
        // Remove Arc from old chunk
        let entity = self
            .chunk_map
            .with_full_chunk(&from, |chunk| {
                chunk.as_full().and_then(|c| c.entities.remove(entity_id))
            })
            .flatten();

        // Add Arc to new chunk
        if let Some(entity) = entity {
            self.chunk_map.with_full_chunk(&to, |chunk| {
                if let Some(c) = chunk.as_full() {
                    c.entities.add(entity);
                }
            });
        }
    }

    /// Internal method to remove an entity from the world.
    ///
    /// Called by `EntityChunkCallback::on_remove`.
    pub fn remove_entity_internal(
        &self,
        entity_id: i32,
        chunk_pos: ChunkPos,
        reason: RemovalReason,
    ) {
        // Remove from chunk storage
        let entity: Option<SharedEntity> = self
            .chunk_map
            .with_full_chunk(&chunk_pos, |chunk| {
                chunk.as_full().and_then(|c| c.entities.remove(entity_id))
            })
            .flatten();

        // Unregister from cache
        if let Some(entity) = entity {
            let pos = entity.position();
            let section = SectionPos::new(
                (pos.x as i32) >> 4,
                (pos.y as i32) >> 4,
                (pos.z as i32) >> 4,
            );
            self.entity_cache
                .unregister(entity_id, entity.uuid(), section);

            // Broadcast remove packet if entity was destroyed
            if reason.should_destroy() {
                let packet = CRemoveEntities::single(entity_id);
                self.broadcast_to_nearby(chunk_pos, packet, None);
            }
        }
    }
}
