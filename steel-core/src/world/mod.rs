//! This module contains the `World` struct, which represents a world.
use std::{
    io,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use sha2::{Digest, Sha256};
use steel_protocol::packet_traits::EncodedPacket;
use steel_protocol::packets::game::{
    CBlockDestruction, CPlayerChat, CPlayerInfoUpdate, CSystemChat,
};
use steel_protocol::utils::ConnectionProtocol;

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::game_rules::{GameRuleRef, GameRuleValue};
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_game_rules::RANDOM_TICK_SPEED;
use steel_registry::{REGISTRY, dimension_type::DimensionTypeRef};

use steel_registry::blocks::shapes::{AABBd, VoxelShape};
use steel_utils::locks::SyncRwLock;
use steel_utils::{BlockPos, BlockStateId, ChunkPos, SectionPos, types::UpdateFlags};
use tokio::{runtime::Runtime, time::Instant};

use crate::{
    ChunkMap,
    behavior::BLOCK_BEHAVIORS,
    config::STEEL_CONFIG,
    level_data::LevelDataManager,
    player::{LastSeen, Player},
};

mod player_area_map;
mod player_map;
mod world_entities;

pub use player_area_map::PlayerAreaMap;
pub use player_map::PlayerMap;

/// Interval in ticks between player info broadcasts (600 ticks = 30 seconds).
/// Matches vanilla `PlayerList.SEND_PLAYER_INFO_INTERVAL`.
const SEND_PLAYER_INFO_INTERVAL: u64 = 600;

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
}

impl World {
    /// Creates a new world.
    ///
    /// Uses `Arc::new_cyclic` to create a cyclic reference between
    /// the World and its `ChunkMap`'s `WorldGenContext`.
    #[allow(clippy::new_without_default)]
    pub async fn new(
        chunk_runtime: Arc<Runtime>,
        dimension: DimensionTypeRef,
        seed: i64,
    ) -> io::Result<Arc<Self>> {
        let level_data =
            LevelDataManager::new(format!("world/{}", dimension.key.path), seed).await?;

        Ok(Arc::new_cyclic(|weak_self: &Weak<World>| Self {
            chunk_map: Arc::new(ChunkMap::new(chunk_runtime, weak_self.clone(), &dimension)),
            players: PlayerMap::new(),
            player_area_map: PlayerAreaMap::new(),
            dimension,
            level_data: SyncRwLock::new(level_data),
            tick_runs_normally: AtomicBool::new(true),
        }))
    }

    /// Cleans up the world by saving all chunks.
    /// `await_holding_lock` is safe here cause it's only done on shutdown
    #[allow(clippy::await_holding_lock)]
    pub async fn cleanup(&self, total_saved: &mut usize) {
        match self.level_data.write().save_force().await {
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
    pub fn get_height(&self) -> i32 {
        self.dimension.height
    }

    /// Returns the minimum Y coordinate of the world.
    pub fn get_min_y(&self) -> i32 {
        self.dimension.min_y
    }

    /// Returns the maximum Y coordinate of the world.
    pub fn get_max_y(&self) -> i32 {
        self.get_min_y() + self.get_height() - 1
    }

    /// Returns whether the given Y coordinate is outside the build height.
    pub fn is_outside_build_height(&self, block_y: i32) -> bool {
        block_y < self.get_min_y() || block_y > self.get_max_y()
    }

    /// Returns whether the block position is within valid horizontal bounds.
    pub fn is_in_valid_bounds_horizontal(&self, block_pos: &BlockPos) -> bool {
        let chunk_x = SectionPos::block_to_section_coord(block_pos.0.x);
        let chunk_z = SectionPos::block_to_section_coord(block_pos.0.z);
        ChunkPos::is_valid(chunk_x, chunk_z)
    }

    /// Returns whether the block position is within valid world bounds.
    pub fn is_in_valid_bounds(&self, block_pos: &BlockPos) -> bool {
        !self.is_outside_build_height(block_pos.0.y)
            && self.is_in_valid_bounds_horizontal(block_pos)
    }

    /// Returns the maximum build height (one above the highest placeable block).
    /// This is `min_y + height`.
    #[must_use]
    pub fn max_build_height(&self) -> i32 {
        self.get_min_y() + self.get_height()
    }

    /// Checks if a player may interact with the world at the given position.
    /// Currently only checks if position is within world bounds.
    #[must_use]
    pub fn may_interact(&self, _player: &Player, pos: &BlockPos) -> bool {
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
    #[must_use]
    pub fn get_game_rule(&self, rule: GameRuleRef) -> GameRuleValue {
        let level_data = self.level_data.read();
        level_data
            .data()
            .game_rules_values
            .get(rule, &REGISTRY.game_rules)
    }

    /// Sets the value of a game rule.
    pub fn set_game_rule(&self, rule: GameRuleRef, value: GameRuleValue) -> bool {
        let mut level_data = self.level_data.write();
        level_data
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
                let (dx, dy, dz) = direction.offset();
                let neighbor_pos = pos.offset(dx, dy, dz);

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
    fn update_neighbors_at(&self, pos: &BlockPos, source_block: BlockRef) {
        for direction in Self::NEIGHBOR_UPDATE_ORDER {
            let (dx, dy, dz) = direction.offset();
            let neighbor_pos = pos.offset(dx, dy, dz);
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

    fn chunk_pos_for_block(pos: &BlockPos) -> ChunkPos {
        ChunkPos::new(
            SectionPos::block_to_section_coord(pos.0.x),
            SectionPos::block_to_section_coord(pos.0.z),
        )
    }

    /// Ticks the world.
    ///
    /// * `tick_count` - The current tick number
    /// * `runs_normally` - Whether game elements (random ticks, entities) should run.
    ///   When false (frozen), only essential operations like chunk loading run.
    pub fn tick_b(&self, tick_count: u64, runs_normally: bool) {
        let random_tick_speed = self.get_game_rule(RANDOM_TICK_SPEED).as_int().unwrap_or(3) as u32;

        self.chunk_map
            .tick_b(tick_count, random_tick_speed, runs_normally);

        // Tick players (always tick players - they can move when frozen)
        let start = Instant::now();
        self.players.iter_players(|_uuid, player| {
            player.tick();

            true
        });
        let player_tick_elapsed = start.elapsed();
        if player_tick_elapsed >= Duration::from_millis(100) {
            log::warn!("Player tick slow: {player_tick_elapsed:?}");
        }

        // Broadcast player latency updates periodically
        if tick_count.is_multiple_of(SEND_PLAYER_INFO_INTERVAL) {
            self.broadcast_player_latency_updates();
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

            self.players.iter_players(|_, player| {
                player.connection.send_packet(packet.clone());
                true
            });
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
            recipient.connection.send_packet(packet.clone());

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
        self.players.iter_players(|_, player| {
            player.connection.send_packet(packet.clone());
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

            recipient.connection.send_packet(packet.clone());
            true
        });
    }

    /// Broadcasts an encoded packet to all players tracking the given chunk.
    pub fn broadcast_to_nearby(
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
                player.connection.send_encoded_packet(packet.clone());
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
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, STEEL_CONFIG.compression, ConnectionProtocol::Play)
        else {
            log::warn!("Failed to encode block destruction packet");
            return;
        };
        self.broadcast_to_nearby(chunk, encoded, Some(entity_id));
    }
}
