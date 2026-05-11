//! This module contains all things player-related.
mod abilities;
pub mod block_breaking;
mod chat_state;
pub mod chunk_sender;
/// This module contains the `PlayerConnection` trait that abstracts network connections.
pub mod connection;
mod entity_state;
/// Experience System
pub mod experience;
pub mod food_data;
/// Game mode specific logic for player interactions.
pub mod game_mode;
mod game_profile;
mod health_sync;
pub mod message_chain;
mod message_validator;
pub mod movement;
mod movement_state;
/// This module contains the networking implementation for the player.
pub mod networking;
pub mod player_data;
pub mod player_data_storage;
pub mod player_inventory;
pub mod profile_key;
mod signature_cache;
mod teleport_state;

pub use abilities::Abilities;
use chat_state::ChatState;
use entity_state::EntityState;
use food_data::FoodData;
use glam::DVec3;
use health_sync::HealthSyncState;
pub use message_validator::LastSeenMessagesValidator;
use movement_state::MovementState;
pub use signature_cache::{LastSeen, MessageCache};
use steel_protocol::{
    packet_traits::{CompressionInfo, EncodedPacket},
    packets::game::CSetExperience,
};
use teleport_state::TeleportState;

use block_breaking::BlockBreakingManager;
use crossbeam::atomic::AtomicCell;
use enum_dispatch::enum_dispatch;
pub use game_profile::{GameProfile, GameProfileAction};
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU32, Ordering},
};
use steel_protocol::packets::game::{
    CAddEntity, CDamageEvent, CEntityEvent, CHurtAnimation, CPlayerCombatKill, CRemoveEntities,
    CRespawn, CSetEntityData, CSetHealth, CSetHeldSlot, CSetTime, CUpdateAttributes,
    ClientCommandAction,
};
use steel_registry::RegistryEntry;
use steel_registry::blocks::shapes::AABBd;
use steel_registry::entity_data::EntityPose;
use steel_registry::entity_types::EntityTypeRef;
use steel_registry::game_rules::GameRuleValue;
use steel_registry::vanilla_entity_data::PlayerEntityData;
use steel_registry::vanilla_game_rules::{
    ADVANCE_TIME, IMMEDIATE_RESPAWN, KEEP_INVENTORY, SHOW_DEATH_MESSAGES,
};
use steel_registry::{vanilla_attributes, vanilla_entities};
use steel_utils::entity_events::EntityStatus;

use arc_swap::ArcSwap;
use steel_utils::locks::SyncMutex;
use steel_utils::types::{Difficulty, GameType};
use text_components::TextComponent;
use text_components::resolving::TextResolutor;
use text_components::translation::TranslatedMessage;
use text_components::{content::Resolvable, custom::CustomData};
use uuid::Uuid;

use crate::config::RuntimeConfig;
use crate::entity::attribute::AttributeMap;
use crate::entity::damage::DamageSource;
use crate::entity::{
    DEATH_DURATION, Entity, EntityLevelCallback, LivingEntityBase, NullEntityCallback,
    RemovalReason,
};
use crate::inventory::SyncPlayerInv;
use crate::player::experience::Experience;
use crate::player::player_inventory::PlayerInventory;
use crate::server::Server;
use steel_registry::vanilla_damage_types;

use steel_protocol::packets::{
    common::SCustomPayload,
    game::{CContainerClose, CGameEvent, CSystemChat, GameEventType, PreviousMessage},
};
use steel_registry::item_stack::ItemStack;

use steel_utils::BlockPos;

use steel_utils::ChunkPos;

use crate::entity::LivingEntity;

use crate::inventory::{MenuInstance, container::Container, inventory_menu::InventoryMenu};

/// Re-export `PreviousMessage` as `PreviousMessageEntry` for use in `signature_cache`
pub type PreviousMessageEntry = PreviousMessage;

pub use steel_protocol::packets::common::{ChatVisibility, HumanoidArm, ParticleStatus};

/// Client-side settings sent via `SClientInformation` packet.
/// This is stored separately from the packet struct to allow default initialization.
#[derive(Debug, Clone)]
pub struct ClientInformation {
    /// The client's language (e.g., "`en_us`").
    pub language: String,
    /// The client's requested view distance in chunks.
    pub view_distance: u8,
    /// Chat visibility setting.
    pub chat_visibility: ChatVisibility,
    /// Whether chat colors are enabled.
    pub chat_colors: bool,
    /// Bitmask for displayed skin parts.
    pub model_customization: i32,
    /// The player's main hand (left or right).
    pub main_hand: HumanoidArm,
    /// Whether text filtering is enabled.
    pub text_filtering_enabled: bool,
    /// Whether the player appears in the server list.
    pub allows_listing: bool,
    /// Particle rendering setting.
    pub particle_status: ParticleStatus,
}

impl Default for ClientInformation {
    fn default() -> Self {
        Self {
            language: "en_us".to_string(),
            view_distance: 8, // Default client view distance
            chat_visibility: ChatVisibility::Full,
            chat_colors: true,
            model_customization: 0,
            main_hand: HumanoidArm::Right,
            text_filtering_enabled: false,
            allows_listing: true,
            particle_status: ParticleStatus::All,
        }
    }
}

use crate::player::connection::NetworkConnection;

/// Concrete player connection type using `enum_dispatch` for zero-cost dispatch.
///
/// The `Java` variant handles real network connections (hot path),
/// while `Other` uses dynamic dispatch for test connections.
#[enum_dispatch(NetworkConnection)]
pub enum PlayerConnection {
    /// A real Java client connection (zero-cost dispatch).
    Java(JavaConnection),
    /// A dynamic connection for tests or other backends.
    Other(Box<dyn NetworkConnection>),
}

use crate::chunk::player_chunk_view::PlayerChunkView;
use crate::player::chunk_sender::ChunkSender;
use crate::player::networking::JavaConnection;
use crate::portal::TeleportTransition;
use crate::world::World;

/// A struct representing a player.
pub struct Player {
    /// The player's game profile.
    pub gameprofile: GameProfile,
    /// The player's connection (abstracted for testing).
    pub connection: Arc<PlayerConnection>,

    /// The world the player is in.
    pub world: ArcSwap<World>,

    /// Reference to the server (for entity ID generation, etc.).
    pub(crate) server: Weak<Server>,
    /// Runtime configuration shared with the server.
    pub(crate) config: Arc<RuntimeConfig>,

    /// The entity ID assigned to this player.
    pub id: i32,

    /// Whether the player has finished loading the client.
    pub client_loaded: AtomicBool,

    /// The player's position.
    pub position: SyncMutex<DVec3>,
    /// The player's rotation (yaw, pitch).
    pub rotation: AtomicCell<(f32, f32)>,
    /// Movement tracking state
    pub(crate) movement: SyncMutex<MovementState>,

    /// Synchronized entity data (health, pose, flags, etc.) for network sync.
    entity_data: SyncMutex<PlayerEntityData>,

    /// The player's movement speed.
    speed: AtomicCell<f32>,

    /// Entity attribute map (movement speed, max health, gravity, etc.).
    attributes: SyncMutex<AttributeMap>,

    /// The last chunk position of the player.
    pub last_chunk_pos: SyncMutex<ChunkPos>,
    /// The last chunk tracking view of the player.
    pub last_tracking_view: SyncMutex<Option<PlayerChunkView>>,
    /// The chunk sender for the player.
    pub chunk_sender: SyncMutex<ChunkSender>,

    /// The client's settings/information (language, view distance, chat visibility, etc.).
    /// Updated when the client sends `SClientInformation` during config or play phase.
    client_information: SyncMutex<ClientInformation>,

    /// Chat state: message counters, signature cache, validator, session, chain.
    pub chat: SyncMutex<ChatState>,

    /// The player's current game mode (Survival, Creative, Adventure, Spectator)
    pub game_mode: AtomicCell<GameType>,

    /// The player's last game mode
    pub prev_game_mode: AtomicCell<GameType>,

    /// The player's inventory container (shared with `inventory_menu`).
    pub inventory: SyncPlayerInv,

    /// The player's inventory menu (always open, even when `container_id` is 0).
    inventory_menu: SyncMutex<InventoryMenu>,

    /// The currently open menu (None if player inventory is open).
    /// This is separate from `inventory_menu` which is always present.
    open_menu: SyncMutex<Option<Box<dyn MenuInstance>>>,

    /// Counter for generating container IDs (1-100, wraps around).
    container_counter: AtomicU8,

    /// Tracks the last acknowledged block change sequence number.
    ack_block_changes_up_to: AtomicI32,

    /// Pending server-initiated teleport state (ID, position, timeout).
    teleport_state: SyncMutex<TeleportState>,

    /// Local tick counter (incremented each tick).
    tick_count: AtomicI32,

    /// Physical state flags (sleeping, fall flying, on ground).
    pub(crate) entity_state: SyncMutex<EntityState>,

    /// Player abilities (flight, invulnerability, build permissions, speeds, etc.)
    pub abilities: SyncMutex<Abilities>,

    /// Block breaking state machine.
    pub block_breaking: SyncMutex<BlockBreakingManager>,

    /// Shared living-entity fields (`dead`, `invulnerable_time`, `last_hurt`).
    /// Vanilla: `LivingEntity` (L230-232) + `Entity.invulnerableTime` (L256).
    living_base: SyncMutex<LivingEntityBase>,

    /// Player food/hunger state (food level, saturation, exhaustion).
    food_data: SyncMutex<FoodData>,

    /// Delta-tracking state for `CSetHealth` deduplication.
    health_sync: SyncMutex<HealthSyncState>,

    /// Whether the player has been removed from the world.
    removed: AtomicBool,
    /// Whether the player is between domain saves/loads and should ignore gameplay packets.
    domain_switching: AtomicBool,

    /// Callback for entity lifecycle events (movement between chunks, removal).
    level_callback: SyncMutex<Arc<dyn EntityLevelCallback>>,

    /// The Player's Experience
    pub experience: SyncMutex<Experience>,

    /// Monotonic counter bumped on world teleport/reset. The chunk sending tick
    /// snapshots this before encoding and compares after to detect stale batches.
    pub chunk_send_epoch: AtomicU32,
}

impl Player {
    /// Computes the start (eye position) and end positions for a raytrace.
    pub fn get_ray_endpoints(&self) -> (DVec3, DVec3) {
        let pos = self.position();
        let start_pos = DVec3::new(pos.x, self.get_eye_y(), pos.z);
        let (yaw, pitch) = self.rotation();
        let (yaw_rad, pitch_rad) = (f64::from(yaw.to_radians()), f64::from(pitch.to_radians()));
        let block_interaction_range = self
            .attributes
            .lock()
            .get_value(vanilla_attributes::BLOCK_INTERACTION_RANGE)
            .unwrap_or(4.5);
        let direction = DVec3::new(
            -yaw_rad.sin() * pitch_rad.cos() * block_interaction_range,
            -pitch_rad.sin() * block_interaction_range,
            pitch_rad.cos() * yaw_rad.cos() * block_interaction_range,
        );

        let end_pos = start_pos + direction;
        (start_pos, end_pos)
    }

    /// Creates a new player.
    #[expect(clippy::too_many_arguments, reason = "Player::new is complex")]
    pub fn new(
        gameprofile: GameProfile,
        connection: Arc<PlayerConnection>,
        world: Arc<World>,
        server: Weak<Server>,
        config: Arc<RuntimeConfig>,
        entity_id: i32,
        player: &Weak<Player>,
        client_information: ClientInformation,
    ) -> Self {
        // Create a single shared inventory container used by both the player and inventory menu
        let inventory = Arc::new(SyncMutex::new(PlayerInventory::new(player.clone())));

        let pos = DVec3::new(0.0, 0.0, 0.0);

        let attributes = AttributeMap::new_for_entity(&vanilla_entities::PLAYER);
        let max_health = attributes
            .get_value(vanilla_attributes::MAX_HEALTH)
            .unwrap_or(20.0) as f32;
        let speed = attributes
            .get_value(vanilla_attributes::MOVEMENT_SPEED)
            .unwrap_or(0.1) as f32;

        Self {
            gameprofile,
            connection,

            world: ArcSwap::new(world),
            server,
            config,
            id: entity_id,
            client_loaded: AtomicBool::new(false),
            position: SyncMutex::new(pos),
            rotation: AtomicCell::new((0.0, 0.0)),
            movement: SyncMutex::new(MovementState::new()),
            entity_data: SyncMutex::new({
                let mut data = PlayerEntityData::new();
                data.health.set(max_health);
                data
            }),
            speed: AtomicCell::new(speed),
            attributes: SyncMutex::new(attributes),
            last_chunk_pos: SyncMutex::new(ChunkPos::new(0, 0)),
            last_tracking_view: SyncMutex::new(None),
            chunk_sender: SyncMutex::new(ChunkSender::default()),
            client_information: SyncMutex::new(client_information),
            chat: SyncMutex::new(ChatState::new()),
            game_mode: AtomicCell::new(GameType::Survival),
            prev_game_mode: AtomicCell::new(GameType::Survival),
            inventory: inventory.clone(),
            inventory_menu: SyncMutex::new(InventoryMenu::new(inventory)),
            open_menu: SyncMutex::new(None),
            container_counter: AtomicU8::new(0),
            ack_block_changes_up_to: AtomicI32::new(-1),
            teleport_state: SyncMutex::new(TeleportState::new()),
            tick_count: AtomicI32::new(0),
            entity_state: SyncMutex::new(EntityState::new()),
            abilities: SyncMutex::new(Abilities::default()),
            block_breaking: SyncMutex::new(BlockBreakingManager::new()),
            living_base: SyncMutex::new(LivingEntityBase::new()),
            food_data: SyncMutex::new(FoodData::new()),
            health_sync: SyncMutex::new(HealthSyncState::new()),
            removed: AtomicBool::new(false),
            domain_switching: AtomicBool::new(false),
            level_callback: SyncMutex::new(Arc::new(NullEntityCallback)),
            experience: SyncMutex::new(Experience::default()),
            chunk_send_epoch: AtomicU32::new(0),
        }
    }

    /// Ticks the player.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "world coordinates are always within i32 range in a valid Minecraft world"
    )]
    pub fn tick(&self) {
        self.tick_count.fetch_add(1, Ordering::Relaxed);

        // Reset first_good_position to current position at start of tick (vanilla: resetPosition)
        {
            let mut mv = self.movement.lock();
            mv.first_good_position = *self.position.lock();
            mv.known_move_packet_count = mv.received_move_packet_count;
        }

        self.apply_gravity();
        self.tick_ack_block_changes();

        if !self.client_loaded.load(Ordering::Relaxed) {
            //return;
        }

        let current_pos = *self.position.lock();
        let chunk_x = (current_pos.x as i32) >> 4;
        let chunk_z = (current_pos.z as i32) >> 4;
        let chunk_pos = ChunkPos::new(chunk_x, chunk_z);

        *self.last_chunk_pos.lock() = chunk_pos;

        let world = self.get_world();
        world.chunk_map.update_player_status(self);

        {
            let mut living_base = self.living_base.lock();
            if living_base.invulnerable_time > 0 {
                living_base.invulnerable_time -= 1;
            }
        }

        if *self.entity_data.lock().health.get() <= 0.0 {
            self.tick_death();
        } else {
            self.touch_nearby_items();
            self.block_breaking.lock().tick(self, &world);
            self.check_inside_blocks();
            self.check_below_world();

            // TODO: Implement remaining player ticking logic here
            // - Managing game mode specific logic
            // - Updating advancements
            // - Handling falling

            // aiStep in vanilla
            if let Some(speed) = self
                .attributes
                .lock()
                .get_value(vanilla_attributes::MOVEMENT_SPEED)
            {
                self.speed.store(speed as f32);
            }

            self.update_player_attributes();
            self.tick_regeneration();

            if self.is_sprinting() && !self.food_data.lock().has_enough_food() {
                self.set_sprinting(false);
            }
        }

        self.refresh_dirty_attributes();

        self.broadcast_inventory_changes();
        self.update_pose();
        self.update_shared_flags();
        self.sync_entity_data();

        {
            let health = *self.entity_data.lock().health.get();
            let (food, saturation) = {
                let food_data = self.food_data.lock();
                (food_data.food_level, food_data.saturation_level)
            };

            let saturation_zero = saturation == 0.0;

            let mut sync = self.health_sync.lock();
            if sync.needs_update(health, food, saturation_zero) {
                self.send_packet(CSetHealth {
                    health,
                    food,
                    food_saturation: saturation,
                });
                sync.record_sent(health, food, saturation_zero);
            }
        }

        {
            let mut experience = self.experience.lock();

            if experience.dirty {
                self.send_packet(CSetExperience {
                    progress: experience.progress() as f32,
                    level: experience.level(),
                    total_experience: experience.total_points(),
                });
                experience.dirty = false;
            }
        }

        {
            let snapshots = self.attributes.lock().drain_dirty_sync();
            if !snapshots.is_empty() {
                let packet = CUpdateAttributes::new(self.id, snapshots);
                let chunk_pos = *self.last_chunk_pos.lock();
                self.get_world()
                    .broadcast_to_nearby(chunk_pos, packet, None);
            }
        }

        self.connection.tick();
    }

    /// Ticks the death animation timer.
    /// Vanilla: `LivingEntity.tickDeath()` (not overridden by `ServerPlayer`).
    fn tick_death(&self) {
        let death_time = {
            let mut living_base = self.living_base.lock();
            living_base.increment_death_time()
        };

        if death_time >= DEATH_DURATION && !self.is_removed() {
            let world = self.get_world();
            let chunk_pos = *self.last_chunk_pos.lock();
            world.broadcast_to_nearby(
                chunk_pos,
                CEntityEvent {
                    entity_id: self.id,
                    event: EntityStatus::Poof,
                },
                None,
            );

            world.broadcast_to_all(CRemoveEntities::single(self.id));
            self.set_removed(RemovalReason::Killed);
        }
    }

    /// Syncs dirty entity data to nearby players.
    fn sync_entity_data(&self) {
        if let Some(dirty_values) = self.entity_data.lock().pack_dirty() {
            let packet = CSetEntityData::new(self.id, dirty_values);
            let chunk_pos = *self.last_chunk_pos.lock();
            self.get_world()
                .broadcast_to_nearby(chunk_pos, packet, None);
        }
    }

    /// Handles a custom payload packet.
    #[expect(clippy::unused_self, reason = "this is an api function")]
    pub fn handle_custom_payload(&self, packet: SCustomPayload) {
        log::info!("Hello from the other side! {packet:?}");
    }

    /// Handles the end of a client tick.
    #[expect(clippy::unused_self, reason = "this is an api function")]
    pub const fn handle_client_tick_end(&self) {
        //log::info!("Hello from the other side!");
    }

    /// Checks all blocks overlapping the player's AABB and calls `entity_inside`
    /// on each block's behavior (e.g. cactus damage, fire ignition).
    fn check_inside_blocks(&self) {
        use crate::behavior::BLOCK_BEHAVIORS;
        use steel_registry::blocks::block_state_ext::BlockStateExt;

        let world = self.get_world();
        let aabb = self.bounding_box().deflate(1.0E-5);

        let min_x = aabb.min_x.floor() as i32;
        let min_y = aabb.min_y.floor() as i32;
        let min_z = aabb.min_z.floor() as i32;
        let max_x = aabb.max_x.floor() as i32;
        let max_y = aabb.max_y.floor() as i32;
        let max_z = aabb.max_z.floor() as i32;

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                for z in min_z..=max_z {
                    let pos = BlockPos::new(x, y, z);
                    let state = world.get_block_state(pos);
                    if state.is_air() {
                        continue;
                    }
                    let block = state.get_block();
                    let behavior = BLOCK_BEHAVIORS.get_behavior(block);
                    behavior.entity_inside(state, &world, pos, self as &dyn Entity);
                }
            }
        }
    }

    fn check_below_world(&self) {
        let pos = *self.position.lock();
        if pos.y < f64::from(self.get_world().get_min_y() - 64) {
            self.hurt(
                &DamageSource::environment(&vanilla_damage_types::OUT_OF_WORLD),
                4.0,
            );
        }
    }

    /// Main entry point for dealing damage. Returns `true` if damage was applied.
    ///
    /// Vanilla: `LivingEntity.hurtServer()` (with `ServerPlayer` override adding
    /// `PvP` checks before delegating to super). When other living entities are
    /// added, the core logic here should move to a `LivingEntity` trait method.
    pub fn hurt(&self, source: &DamageSource, amount: f32) -> bool {
        // TODO: Vanilla ServerPlayer.hurtServer checks isInvulnerableTo() first, which
        // includes gamerule checks (drowningDamage, fallDamage, fireDamage, freezeDamage).
        // Add when gamerule damage-type system is implemented.

        {
            let abilities = self.abilities.lock();
            if abilities.invulnerable && !source.bypasses_invulnerability() {
                return false;
            }
        }

        if *self.entity_data.lock().health.get() <= 0.0 {
            return false;
        }

        // TODO: Vanilla LivingEntity.hurtServer checks fire resistance effect here:
        //   if source.is(IS_FIRE) && hasEffect(FIRE_RESISTANCE) → return false
        // Blocked on: potion/effect system

        // TODO: Vanilla LivingEntity.hurtServer wakes sleeping entities:
        //   if this.isSleeping() { this.stopSleeping(); }
        // Blocked on: full sleep system (stopSleeping, bed block state, stand-up position)

        // TODO: Vanilla LivingEntity.hurtServer sets this.noActionTime = 0 here.
        // This is the LivingEntity mob-despawn counter (separate from ServerPlayer.lastActionTime).
        // For players it's not critical, but add for completeness when mob AI is implemented.

        // Difficulty scaling (vanilla: Player.hurtServer)
        let mut amount = amount;
        if source.scales_with_difficulty() {
            let difficulty = self.get_world().level_data.read().data().difficulty;
            match difficulty {
                Difficulty::Peaceful => {
                    amount = 0.0;
                }
                Difficulty::Easy => {
                    amount = (amount / 2.0 + 1.0).min(amount);
                }
                Difficulty::Hard => {
                    amount = amount * 3.0 / 2.0;
                }
                Difficulty::Normal => {}
            }
        }

        if amount <= 0.0 {
            return false;
        }

        let (took_full_damage, effective_amount) = {
            let mut living_base = self.living_base.lock();
            if living_base.dead {
                return false;
            }

            if living_base.invulnerable_time > 10 && !source.bypasses_cooldown() {
                if amount <= living_base.last_hurt {
                    return false;
                }
                let effective = amount - living_base.last_hurt;
                living_base.last_hurt = amount;
                (false, effective)
            } else {
                living_base.last_hurt = amount;
                living_base.invulnerable_time = 20;
                (true, amount)
            }
        };

        // TODO: Vanilla LivingEntity.hurtServer applies item blocking (shield) before
        // actuallyHurt via applyItemBlocking(). Implements shield damage reduction,
        // BlocksAttacks component, and shield disable cooldown.
        // Blocked on: item use / blocking system, BlocksAttacks data component

        self.actually_hurt(source, effective_amount);

        // TODO: Vanilla LivingEntity.hurtServer applies knockback after damage:
        //   - Calculates knockback direction from source position or projectile
        //   - Calls this.knockback(0.4, dx, dz)
        //   - Calls this.indicateDamage(dx, dz) if not blocked
        // Blocked on: knockback / velocity system, projectile system

        if took_full_damage {
            let type_id = source.damage_type.id() as i32;
            let chunk_pos = *self.last_chunk_pos.lock();
            let world = self.get_world();

            world.broadcast_to_nearby(
                chunk_pos,
                CDamageEvent {
                    entity_id: self.id,
                    source_type_id: type_id,
                    source_cause_id: source.causing_entity_id.map_or(0, |id| id + 1),
                    source_direct_id: source.direct_entity_id.map_or(0, |id| id + 1),
                    source_position: source.source_position,
                },
                None,
            );

            let (yaw, _) = self.rotation.load();
            world.broadcast_to_nearby(
                chunk_pos,
                CHurtAnimation {
                    entity_id: self.id,
                    yaw,
                },
                None,
            );
        }

        if *self.entity_data.lock().health.get() <= 0.0 {
            self.die(source);
        }

        true
    }

    /// Applies damage after reductions.
    /// TODO: armor, enchantment, absorption
    fn actually_hurt(&self, source: &DamageSource, amount: f32) {
        // TODO: apply armor/enchant/absorption reductions here (vanilla: getDamageAfterArmorAbsorb, getDamageAfterMagicAbsorb)
        // TODO: absorption amount handling
        // TODO: combat tracker (getCombatTracker().recordDamage)
        if amount <= 0.0 {
            return;
        }

        // TODO: absorption handling
        self.cause_food_exhaustion(source.damage_type.exhaustion);

        let mut entity_data = self.entity_data.lock();
        let new_health = (*entity_data.health.get() - amount).max(0.0);
        entity_data.health.set(new_health);
    }

    /// Vanilla: `ServerPlayer.die()` (does NOT call `super.die()`).
    fn die(&self, source: &DamageSource) {
        {
            let mut living_base = self.living_base.lock();
            if self.removed.load(Ordering::Relaxed) || living_base.dead {
                return;
            }

            living_base.dead = true;
        }

        {
            let mut experience = self.experience.lock();

            experience.sync_score(&mut self.entity_data.lock());
            experience.score = 0;
        }

        self.sync_entity_data();

        // NOTE: Vanilla `ServerPlayer.die()` does NOT set Pose::Dying — only
        // `LivingEntity.die()` does (which ServerPlayer never calls via super).
        // The death screen covers the player model, so the pose is irrelevant.

        let world = self.get_world();

        // Broadcast entity event 3 (death sound) to all nearby players.
        let chunk_pos = *self.last_chunk_pos.lock();
        world.broadcast_to_nearby(
            chunk_pos,
            CEntityEvent {
                entity_id: self.id,
                event: EntityStatus::Death,
            },
            None,
        );

        let show_death_messages =
            world.get_game_rule(&SHOW_DEATH_MESSAGES) == GameRuleValue::Bool(true);

        // TODO: use CombatTracker for multi-arg messages (killer name, item, etc.)
        let death_key = format!("death.attack.{}", source.damage_type.message_id);
        let death_message = TranslatedMessage {
            key: death_key.into(),
            fallback: None,
            args: Some(Box::new([TextComponent::plain(
                self.gameprofile.name.clone(),
            )])),
        }
        .component();

        self.send_packet(CPlayerCombatKill {
            player_id: self.id,
            message: if show_death_messages {
                death_message.clone()
            } else {
                TextComponent::const_plain("")
            },
        });

        // TODO: team death message visibility (ALWAYS / HIDE_FOR_OTHER_TEAMS / HIDE_FOR_OWN_TEAM)
        if show_death_messages {
            world.broadcast_system_chat(CSystemChat {
                content: death_message,
                overlay: false,
            });
        }

        if world.get_game_rule(&KEEP_INVENTORY) != GameRuleValue::Bool(true) {
            let items: Vec<ItemStack> = {
                let mut inventory = self.inventory.lock();
                (0..inventory.get_container_size())
                    .filter_map(|slot| {
                        let item = inventory.get_item(slot).clone();
                        if item.is_empty() {
                            None
                        } else {
                            inventory.set_item(slot, ItemStack::empty());
                            Some(item)
                        }
                    })
                    .collect()
            };
            for item in items {
                self.drop_item(item, true, false);
            }
        }

        if world.get_game_rule(&IMMEDIATE_RESPAWN) == GameRuleValue::Bool(true) {
            self.respawn();
        }
    }

    /// TODO: bed/respawn anchor, cross-world, potion clearing, noRespawnBlockAvailable
    ///
    /// # Panics
    /// If the player dies in a world that doesn't exist.
    pub fn respawn(&self) {
        {
            let mut living_base = self.living_base.lock();
            if !living_base.dead {
                return;
            }
            living_base.reset_death_state();
        };

        let was_removed = self.removed.swap(false, Ordering::AcqRel);
        let world = self.get_world();

        // Only send CRemoveEntities if tick_death() hasn't already removed us
        // (tick_death sends CRemoveEntities + set_removed at DEATH_DURATION).
        // NOTE: Since we reuse the same entity ID (unlike vanilla which creates a
        // fresh ServerPlayer), clients may briefly see remove+re-add in the same
        // frame if respawn races with tick_death's DEATH_DURATION removal.
        if !was_removed {
            world.broadcast_to_all(CRemoveEntities::single(self.id));
        }

        // Respawn-specific state: reset health and pose
        {
            let mut entity_data = self.entity_data.lock();
            entity_data.health.set(self.get_max_health());
            entity_data.pose.set(EntityPose::Standing);
        }

        // Reset food data to defaults
        *self.food_data.lock() = FoodData::new();

        // Clear transient attribute modifiers (sprint, potion effects, etc.)
        self.attributes.lock().remove_all_transient();

        self.health_sync.lock().reset_for_respawn();

        // TODO: bed/respawn anchor lookup, send NO_RESPAWN_BLOCK_AVAILABLE if missing

        let Some(player_arc) = world.players.get_by_entity_id(self.id) else {
            return;
        };

        // Shared reset (clears transient state, sends CRespawn)
        player_arc.reset(world.clone(), ResetReason::Respawn);

        // Compute spawn position
        let spawn_pos = world.level_data.read().data().spawn_pos();
        let spawn = DVec3::new(
            f64::from(spawn_pos.x()) + 0.5,
            f64::from(spawn_pos.y()),
            f64::from(spawn_pos.z()) + 0.5,
        );

        // TODO: send CSetDefaultSpawnPosition (dimension, pos, yaw, pitch)

        self.send_difficulty();

        // Handle XP loss on death
        {
            let mut experience = self.experience.lock();
            if world.get_game_rule(&KEEP_INVENTORY) != GameRuleValue::Bool(true)
                && self.game_mode.load() != GameType::Spectator
            {
                // TODO: drop XP orbs (min(level * 7, 100))
                experience.set_total_points(0);
            }
            // Re-send XP to client after respawn regardless of keepInventory
            experience.dirty = true;
        }

        // TODO: send mob effect packets once effects are implemented
        // TODO: send CInitializeBorder once world border is implemented

        // Broadcast respawned entity to other players
        // Vanilla: ChunkMap.addEntity -> addPairing -> sendPairingData
        // TODO: also send SetEquipment + UpdateAttributes in the bundle
        let player_type_id = vanilla_entities::PLAYER.id() as i32;
        let spawn_packet = CAddEntity::player(
            self.id,
            self.gameprofile.id,
            player_type_id,
            spawn.x,
            spawn.y,
            spawn.z,
            0.0,
            0.0,
        );
        let entity_data = self.entity_data.lock().pack_all();
        let entity_id = self.id;
        world.players.iter_players(|_, p| {
            if p.id != entity_id {
                p.send_bundle(|bundle| {
                    bundle.add(spawn_packet.clone());
                    if !entity_data.is_empty() {
                        bundle.add(CSetEntityData::new(entity_id, entity_data.clone()));
                    }
                });
            }
            true
        });

        // Shared spawn (teleport, abilities, weather, time, chunk tracking reset)
        player_arc.spawn(spawn, (0.0, 0.0), ResetReason::Respawn);
    }

    /// Handles client commands, requestStats and `RequestGameRuleValues` are still todo
    pub fn handle_client_command(&self, action: ClientCommandAction) {
        match action {
            ClientCommandAction::PerformRespawn => self.respawn(),
            ClientCommandAction::RequestStats | ClientCommandAction::RequestGameRuleValues => {
                // TODO: implement stats
            }
        }
    }

    /// Cleans up player resources.
    #[expect(clippy::unused_self, reason = "this is an api function")]
    pub const fn cleanup(&self) {}

    /// Returns the world the player is currently in.
    pub fn get_world(&self) -> Arc<World> {
        self.world.load_full()
    }

    /// Returns the server this player belongs to.
    pub(crate) fn server(&self) -> Arc<Server> {
        self.server
            .upgrade()
            .expect("player must not outlive server")
    }

    /// Sets the world the player is in.
    ///
    /// This is used when the correct world isn't known at construction time
    /// (e.g., when loading saved player data determines the actual world).
    pub fn set_world(&self, world: Arc<World>) {
        self.world.store(world);
    }

    /// Marks the player as switching domains if they are not already in a transition.
    pub fn begin_domain_switch(&self) -> bool {
        self.domain_switching
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Clears the domain-switch transition marker.
    pub fn finish_domain_switch(&self) {
        self.domain_switching.store(false, Ordering::Release);
    }

    /// Returns whether this player is currently switching domains.
    pub fn is_domain_switching(&self) -> bool {
        self.domain_switching.load(Ordering::Acquire)
    }

    /// Resets the player's transient state and prepares them for a new world.
    ///
    /// This is the shared "clean slate" path used by initial join, respawn, and
    /// world change. If the player is currently in a different world, they are
    /// removed from the old world first.
    ///
    /// Vanilla equivalent: the work that happens when a fresh `ServerPlayer` is
    /// constructed during respawn / world change, since vanilla recreates the
    /// player object. We reuse the same `Player`, so we reset manually.
    pub fn reset(self: &Arc<Self>, new_world: Arc<World>, reason: ResetReason) {
        let old_world = self.get_world();
        let switching_worlds = !Arc::ptr_eq(&old_world, &new_world);

        // --- Old world cleanup (only when actually switching worlds) ---
        if switching_worlds {
            self.do_close_container();
            self.send_packet(CContainerClose { container_id: 0 });
            old_world.remove_player_for_world_change(self);
            self.set_world(new_world.clone());
        }

        // --- Reset transient state ---
        self.client_loaded.store(false, Ordering::Relaxed);
        self.movement.lock().delta_movement = DVec3::default();
        {
            let mut es = self.entity_state.lock();
            es.on_ground = false;
            es.fall_flying = false;
            es.sleeping = false;
            es.crouching = false;
            es.sprinting = false;
        }
        *self.block_breaking.lock() = BlockBreakingManager::new();

        // Reset chunk tracking — bump generation counter so the chunk sending tick
        // discards any in-flight batch encoded against the old world.
        self.chunk_send_epoch.fetch_add(1, Ordering::Release);
        *self.chunk_sender.lock() = ChunkSender::default();
        *self.last_tracking_view.lock() = None;
        *self.last_chunk_pos.lock() = ChunkPos::new(i32::MAX, i32::MAX);

        // --- Send CRespawn (not needed on initial join — CLogin already sent) ---
        if reason != ResetReason::InitialJoin {
            // 0x01 = keep attributes, 0x02 = keep entity data
            let data_kept: i8 = match reason {
                ResetReason::WorldChange => 0x03,
                _ => 0x00,
            };

            self.send_packet(CRespawn {
                dimension_type: new_world.dimension_type.id() as i32,
                dimension_name: new_world.key.clone(),
                hashed_seed: new_world.obfuscated_seed(),
                gamemode: self.game_mode.load() as u8,
                previous_gamemode: self.prev_game_mode.load() as i8,
                is_debug: false,
                is_flat: new_world.is_flat,
                has_death_location: false,
                death_dimension_name: None,
                death_location: None,
                portal_cooldown_ticks: 0,
                sea_level: new_world.sea_level,
                data_kept,
            });
        }
    }

    /// Spawns the player into their current world at the given position.
    ///
    /// This is the shared "enter world" path used by initial join, respawn, and
    /// world change. Sends position sync, abilities, inventory, time, weather,
    /// and adds the player to the world as appropriate for the given reason.
    ///
    /// # Panics
    /// Panics if the `advance_time` gamerule is not a bool.
    pub fn spawn(self: &Arc<Self>, position: DVec3, rotation: (f32, f32), reason: ResetReason) {
        let world = self.get_world();

        // Set position and rotation
        *self.position.lock() = position;
        self.rotation.store(rotation);
        {
            let mut mv = self.movement.lock();
            mv.prev_position = position;
            mv.last_good_position = position;
            mv.first_good_position = position;
            mv.received_move_packet_count = 0;
            mv.known_move_packet_count = 0;
        }

        // Teleport sync (sends CPlayerPosition, sets awaiting_teleport for ack)
        self.teleport(position.x, position.y, position.z, rotation.0, rotation.1);

        // Abilities and held slot
        self.send_abilities();
        self.send_packet(CSetHeldSlot {
            slot: i32::from(self.inventory.lock().get_selected_slot()),
        });

        // Time sync
        {
            let level_data = world.level_data.read();
            let game_time = level_data.game_time();
            let day_time = level_data.day_time();
            drop(level_data);

            let advance_time = world
                .get_game_rule(&ADVANCE_TIME)
                .as_bool()
                .expect("gamerule advance_time should always be a bool.");
            let rate = if advance_time { 1.0 } else { 0.0 };
            self.send_packet(CSetTime::new(game_time, day_time, 0.0, rate));
        }

        // Weather sync
        if world.can_have_weather() && world.is_raining() {
            let (rain_level, thunder_level) = {
                let weather = world.weather.lock();
                (weather.rain_level, weather.thunder_level)
            };

            self.send_packet(CGameEvent {
                event: GameEventType::StartRaining,
                data: 0.0,
            });
            self.send_packet(CGameEvent {
                event: GameEventType::RainLevelChange,
                data: rain_level,
            });
            self.send_packet(CGameEvent {
                event: GameEventType::ThunderLevelChange,
                data: thunder_level,
            });
        }

        // Force health/xp resync on next tick
        self.reset_sent_info();

        // Resend client context that is not fully covered by CLogin/CRespawn.
        self.server().resend_player_context(self);

        // Add to world / re-enter chunk tracking
        match reason {
            ResetReason::InitialJoin | ResetReason::WorldChange => {
                if reason == ResetReason::WorldChange {
                    log::info!(
                        "Player {} changed world to {}",
                        self.gameprofile.name,
                        world.key
                    );
                }
                world.add_player(self.clone(), reason);
            }
            ResetReason::Respawn => {
                // Same world — re-enter chunk tracking
                world.player_area_map.remove_by_entity_id(self.id);
                world.chunk_map.remove_player(self);
                world.entity_tracker().on_player_leave(self.id);

                self.send_packet(CGameEvent {
                    event: GameEventType::LevelChunksLoadStart,
                    data: 0.0,
                });
            }
        }
    }
}

/// Why the player is being reset and spawned into a world.
///
/// Controls which packets are sent and how world add/remove is handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetReason {
    /// First time joining the server. `CLogin` was already sent, so `CRespawn` is skipped.
    InitialJoin,
    /// Respawning after death in the same world.
    Respawn,
    /// Teleporting to a different loaded world.
    WorldChange,
}

impl Entity for Player {
    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::PLAYER
    }

    fn id(&self) -> i32 {
        self.id
    }

    fn uuid(&self) -> Uuid {
        self.gameprofile.id
    }

    fn position(&self) -> DVec3 {
        *self.position.lock()
    }

    fn bounding_box(&self) -> AABBd {
        let pos = self.position();
        // Player hitbox: 0.6 wide, 1.8 tall (standing)
        // TODO: Adjust for pose (crouching, swimming, etc.)
        let half_width = 0.3;
        let height = 1.8;
        AABBd {
            min_x: pos.x - half_width,
            min_y: pos.y,
            min_z: pos.z - half_width,
            max_x: pos.x + half_width,
            max_y: pos.y + height,
            max_z: pos.z + half_width,
        }
    }

    fn tick(&self) {
        // Player tick is handled separately by World::tick_game()
        // This is here for Entity trait compliance
    }

    fn level(&self) -> Option<Arc<World>> {
        Some(self.world.load_full())
    }

    fn is_removed(&self) -> bool {
        self.removed.load(Ordering::Relaxed)
    }

    fn set_removed(&self, reason: RemovalReason) {
        if !self.removed.swap(true, Ordering::AcqRel) {
            self.level_callback.lock().on_remove(reason);
        }
    }

    fn set_level_callback(&self, callback: Arc<dyn EntityLevelCallback>) {
        *self.level_callback.lock() = callback;
    }

    fn as_player(self: Arc<Self>) -> Option<Arc<Player>> {
        Some(self)
    }

    fn rotation(&self) -> (f32, f32) {
        self.rotation.load()
    }

    fn velocity(&self) -> DVec3 {
        self.movement.lock().delta_movement
    }

    fn on_ground(&self) -> bool {
        self.entity_state.lock().on_ground
    }

    /// Returns the eye height for the current pose.
    ///
    /// Vanilla eye heights from `Avatar.POSES`:
    /// - Standing: 1.62
    /// - Crouching: 1.27
    /// - Swimming/FallFlying/SpinAttack: 0.4
    /// - Sleeping: 0.2
    fn get_eye_height(&self) -> f64 {
        match self.get_desired_pose() {
            EntityPose::Sneaking => 1.27,
            EntityPose::FallFlying | EntityPose::Swimming | EntityPose::SpinAttack => 0.4,
            EntityPose::Sleeping => 0.2,
            // Standing and all other poses use default player eye height
            _ => f64::from(vanilla_entities::PLAYER.dimensions.eye_height),
        }
    }

    fn hurt(&self, source: &DamageSource, amount: f32) -> bool {
        // Delegates to Player's inherent hurt method which handles
        // invulnerability, armor, death, and network packets.
        Player::hurt(self, source, amount)
    }

    fn change_world(self: Arc<Self>, teleport_transition: &TeleportTransition) {
        let new_world = teleport_transition.target_world.clone();
        if Arc::ptr_eq(&self.get_world(), &new_world) {
            let pos = teleport_transition.position;
            let rotation = teleport_transition.rotation;
            self.teleport(pos.x, pos.y, pos.z, rotation.0, rotation.1);
        } else {
            self.reset(new_world, ResetReason::WorldChange);
            // TODO: set portal cooldown from teleport_transition.portal_cooldown
            self.spawn(
                teleport_transition.position,
                teleport_transition.rotation,
                ResetReason::WorldChange,
            );
            // Vanilla: PlayerList.sendAllPlayerInfo -> inventoryMenu.sendAllDataToRemote
            self.send_inventory_to_remote();
        }
    }
}

impl LivingEntity for Player {
    fn attributes(&self) -> &SyncMutex<AttributeMap> {
        &self.attributes
    }

    fn get_health(&self) -> f32 {
        *self.entity_data.lock().health.get()
    }

    fn set_health(&self, health: f32) {
        let max_health = self.get_max_health();
        let clamped = health.clamp(0.0, max_health);
        self.entity_data.lock().health.set(clamped);
    }

    fn living_base(&self) -> &SyncMutex<LivingEntityBase> {
        &self.living_base
    }

    fn get_absorption_amount(&self) -> f32 {
        *self.entity_data.lock().player_absorption.get()
    }

    fn set_absorption_amount(&self, amount: f32) {
        self.entity_data
            .lock()
            .player_absorption
            .set(amount.max(0.0));
    }

    fn is_sprinting(&self) -> bool {
        self.entity_state.lock().sprinting
    }

    fn set_sprinting(&self, sprinting: bool) {
        self.entity_state.lock().sprinting = sprinting;
        self.apply_sprint_speed_modifier(sprinting);
    }

    fn get_speed(&self) -> f32 {
        self.speed.load()
    }

    fn set_speed(&self, speed: f32) {
        self.speed.store(speed);
    }
}

impl TextResolutor for Player {
    fn resolve_content(&self, _resolvable: &Resolvable) -> TextComponent {
        TextComponent::new()
    }

    fn resolve_custom(&self, _data: &CustomData) -> Option<TextComponent> {
        None
    }

    fn translate(&self, _key: &str) -> Option<String> {
        None
    }
}
