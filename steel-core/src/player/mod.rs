//! This module contains all things player-related.
mod abilities;
pub mod block_breaking;
mod chat_state;
pub mod chunk_sender;
/// This module contains the `PlayerConnection` trait that abstracts network connections.
pub mod connection;
mod container_counter;
mod entity_state;
/// Experience System
pub mod experience;
pub mod food_data;
/// Game mode specific logic for player interactions.
pub mod game_mode;
mod game_mode_state;
mod game_profile;
mod health_sync;
mod input_state;
mod lifecycle_state;
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
mod tick_state;

pub use abilities::Abilities;
use chat_state::ChatState;
use container_counter::ContainerCounter;
use food_data::FoodData;
use glam::DVec3;
use health_sync::HealthSyncState;
pub use input_state::PlayerInput;
use lifecycle_state::PlayerLifecycleState;
pub use message_validator::LastSeenMessagesValidator;
use movement_state::MovementState;
pub use signature_cache::{LastSeen, MessageCache};
use steel_protocol::{
    packet_traits::{CompressionInfo, EncodedPacket},
    packets::game::{CSetEntityData, CSetExperience},
};
use teleport_state::TeleportState;
use tick_state::PlayerTickState;

use block_breaking::BlockBreakingManager;
use enum_dispatch::enum_dispatch;
use game_mode_state::PlayerGameModeState;
pub use game_profile::{GameProfile, GameProfileAction};
use std::sync::{Arc, Weak};
use steel_protocol::packets::game::{
    AttributeSnapshot, CDamageEvent, CEntityEvent, CHurtAnimation, CPlayerCombatKill, CRespawn,
    CSetHealth, CSetHeldSlot, CSetTime, ClientCommandAction, SoundSource,
};
use steel_registry::RegistryEntry;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_data::EntityPose;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::game_rules::GameRuleValue;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entity_data::PlayerEntityData;
use steel_registry::vanilla_game_rules::{
    ADVANCE_TIME, IMMEDIATE_RESPAWN, KEEP_INVENTORY, MAX_ENTITY_CRAMMING, SHOW_DEATH_MESSAGES,
};
use steel_registry::{sound_events, vanilla_attributes, vanilla_entities};
use steel_utils::entity_events::EntityStatus;

use arc_swap::ArcSwap;
use steel_utils::locks::SyncMutex;
use steel_utils::random::Random as _;
use steel_utils::types::{Difficulty, GameType};
use text_components::TextComponent;
use text_components::resolving::TextResolutor;
use text_components::translation::TranslatedMessage;
use text_components::{content::Resolvable, custom::CustomData};

use crate::config::RuntimeConfig;
use crate::entity::damage::DamageSource;
use crate::entity::{
    DEATH_DURATION, Entity, EntityBase, EntitySyncedData, LivingEntity, LivingEntityBase,
    RemovalReason, SharedEntity,
};
use crate::fluid::get_fluid_state;
use crate::inventory::{SyncPlayerInv, equipment::EquipmentSlot};
use crate::physics::MoveResult;
use crate::player::experience::Experience;
use crate::player::player_data::PersistentRootVehicle;
use crate::player::player_inventory::PlayerInventory;
use crate::server::Server;
use steel_registry::vanilla_damage_types;

use steel_protocol::packets::{
    common::SCustomPayload,
    game::{CContainerClose, CGameEvent, CSystemChat, GameEventType, PreviousMessage},
};
use steel_registry::item_stack::ItemStack;

use steel_utils::{BlockPos, BlockStateId, ChunkPos, Identifier};

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

    /// Common entity fields (id, uuid, position, rotation, removal, callback).
    base: EntityBase,

    /// Client lifecycle flags.
    lifecycle: SyncMutex<PlayerLifecycleState>,

    /// Movement tracking state
    pub(crate) movement: SyncMutex<MovementState>,

    /// Synchronized entity data (health, pose, flags, etc.) for network sync.
    entity_data: SyncMutex<PlayerEntityData>,

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

    /// Current and previous game mode.
    game_modes: SyncMutex<PlayerGameModeState>,

    /// The player's inventory container (shared with `inventory_menu`).
    pub inventory: SyncPlayerInv,

    /// The player's inventory menu (always open, even when `container_id` is 0).
    inventory_menu: SyncMutex<InventoryMenu>,

    /// The currently open menu (None if player inventory is open).
    /// This is separate from `inventory_menu` which is always present.
    open_menu: SyncMutex<Option<Box<dyn MenuInstance>>>,

    /// Counter for generating container IDs (1-100, wraps around).
    container_counter: SyncMutex<ContainerCounter>,

    /// Pending server-initiated teleport state (ID, position, timeout).
    teleport_state: SyncMutex<TeleportState>,

    /// Local tick and once-per-tick packet state.
    tick_state: SyncMutex<PlayerTickState>,

    /// Player abilities (flight, invulnerability, build permissions, speeds, etc.)
    pub abilities: SyncMutex<Abilities>,

    /// Block breaking state machine.
    pub block_breaking: SyncMutex<BlockBreakingManager>,

    /// Shared living-entity runtime fields (attributes, speed, damage/death state).
    /// Vanilla: `LivingEntity` (L230-232) + `Entity.invulnerableTime` (L256).
    living_base: LivingEntityBase,

    /// Player food/hunger state (food level, saturation, exhaustion).
    pub food_data: SyncMutex<FoodData>,

    /// Delta-tracking state for `CSetHealth` deduplication.
    health_sync: SyncMutex<HealthSyncState>,

    /// The Player's Experience
    pub experience: SyncMutex<Experience>,

    /// Monotonic counter bumped on world teleport/reset. The chunk sending tick
    /// snapshots this before encoding and compares after to detect stale batches.
    pub chunk_send_epoch: SyncMutex<u32>,

    /// Persisted `RootVehicle` payload awaiting live entity restoration.
    pending_root_vehicle: SyncMutex<Option<PendingRootVehicleRestore>>,
}

#[derive(Clone)]
struct PendingRootVehicleRestore {
    world: Identifier,
    root_vehicle: PersistentRootVehicle,
}

impl Player {
    /// Computes the start (eye position) and end positions for a raytrace.
    pub fn get_ray_endpoints(&self) -> (DVec3, DVec3) {
        let pos = self.position();
        let start_pos = DVec3::new(pos.x, self.get_eye_y(), pos.z);
        let block_interaction_range = self
            .attributes()
            .lock()
            .get_value(vanilla_attributes::BLOCK_INTERACTION_RANGE)
            .unwrap_or(4.5);
        let direction = self.look_angle() * block_interaction_range;

        let end_pos = start_pos + direction;
        (start_pos, end_pos)
    }

    /// Returns the player's current game mode.
    #[must_use]
    pub fn game_mode(&self) -> GameType {
        self.game_modes.lock().current()
    }

    /// Returns the player's previous game mode.
    #[must_use]
    pub fn previous_game_mode(&self) -> GameType {
        self.game_modes.lock().previous()
    }

    /// Restores current and previous game mode from persistent player data.
    pub(crate) fn restore_game_modes(&self, current: GameType, previous: GameType) {
        self.game_modes.lock().set_pair(current, previous);
    }

    /// Changes the current game mode and records the old current mode as previous.
    fn change_game_mode_state(&self, game_mode: GameType) -> bool {
        self.game_modes.lock().change_current(game_mode)
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

        let living_base = LivingEntityBase::new(&vanilla_entities::PLAYER);
        let player_uuid = gameprofile.id;
        let world_ref = Arc::downgrade(&world);

        Self {
            gameprofile,
            connection,

            world: ArcSwap::new(world),
            server,
            config,
            base: EntityBase::with_uuid(
                entity_id,
                player_uuid,
                pos,
                Self::dimensions_for_pose(EntityPose::Standing),
                world_ref,
            ),
            lifecycle: SyncMutex::new(PlayerLifecycleState::default()),
            movement: SyncMutex::new(MovementState::new()),
            entity_data: SyncMutex::new({
                let mut data = PlayerEntityData::new();
                living_base.initialize_synced_data(&mut data);
                data
            }),
            last_chunk_pos: SyncMutex::new(ChunkPos::new(0, 0)),
            last_tracking_view: SyncMutex::new(None),
            chunk_sender: SyncMutex::new(ChunkSender::default()),
            client_information: SyncMutex::new(client_information),
            chat: SyncMutex::new(ChatState::new()),
            game_modes: SyncMutex::new(PlayerGameModeState::new(GameType::Survival)),
            inventory: inventory.clone(),
            inventory_menu: SyncMutex::new(InventoryMenu::new(inventory)),
            open_menu: SyncMutex::new(None),
            container_counter: SyncMutex::new(ContainerCounter::new()),
            teleport_state: SyncMutex::new(TeleportState::new()),
            tick_state: SyncMutex::new(PlayerTickState::new()),
            abilities: SyncMutex::new(Abilities::default()),
            block_breaking: SyncMutex::new(BlockBreakingManager::new()),
            living_base,
            food_data: SyncMutex::new(FoodData::new()),
            health_sync: SyncMutex::new(HealthSyncState::new()),
            experience: SyncMutex::new(Experience::default()),
            chunk_send_epoch: SyncMutex::new(0),
            pending_root_vehicle: SyncMutex::new(None),
        }
    }

    /// Ticks the player.
    ///
    /// # Panics
    ///
    /// Panics if the player position cannot be restored after `ai_step`. Vanilla treats the
    /// pre-tick position as authoritative here, so a rejection indicates corrupted entity state.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "world coordinates are always within i32 range in a valid Minecraft world"
    )]
    pub fn tick(&self) {
        self.advance_tick();
        self.tick_client_load_timeout();
        if !self.is_passenger() {
            self.advance_tick_count();
        }

        self.set_no_physics(self.is_spectator());
        if self.is_spectator() || self.is_passenger() {
            self.set_on_ground(false);
        }

        let tick_position = self.position();

        // Vanilla: ServerGamePacketListenerImpl.resetPosition().
        self.movement.lock().reset_for_tick(tick_position);
        self.set_old_position_to_current();
        self.reset_vehicle_movement_for_tick();

        self.default_tick();
        self.update_swimming();
        self.ai_step();

        // Vanilla snaps the player back to firstGood after ServerPlayer.doTick().
        if let Err(error) = self.try_set_position(tick_position) {
            panic!(
                "failed to restore player {} tick position after ai_step: {error}",
                self.id()
            );
        }
        self.refresh_fluid_contact();

        self.tick_ack_block_changes();

        if !self.has_client_loaded() {
            //return;
        }

        let world = self.get_world();
        world.chunk_map.update_player_status(self);

        self.living_base.decrement_invulnerable_time();

        if self.get_health() <= 0.0 {
            self.tick_death();
        } else {
            self.touch_nearby_items();
            self.block_breaking.lock().tick(self, &world);
            self.apply_effects_from_blocks();
            self.push_entities(&world);

            // TODO: Implement remaining player ticking logic here
            // - Managing game mode specific logic
            // - Updating advancements
            // - Handling falling

            // aiStep in vanilla
            self.living_base.refresh_speed_from_attributes();

            self.update_player_attributes();
            self.tick_regeneration();

            if self.is_sprinting() && !self.food_data.lock().has_enough_food() {
                self.set_sprinting(false);
            }
        }

        if self.disconnect_if_floating_too_long() {
            return;
        }
        if self.disconnect_if_vehicle_floating_too_long() {
            return;
        }

        self.refresh_dirty_attributes();
        self.tick_living_state();

        self.broadcast_inventory_changes();
        self.update_pose();

        {
            let health = self.get_health();
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

        self.connection.tick();
    }

    /// Ticks the death animation timer.
    /// Vanilla: `LivingEntity.tickDeath()` (not overridden by `ServerPlayer`).
    fn tick_death(&self) {
        let death_time = self.living_base.increment_death_time();

        if death_time >= DEATH_DURATION && !self.is_removed() {
            let world = self.get_world();
            let chunk_pos = *self.last_chunk_pos.lock();
            world.broadcast_to_nearby(
                chunk_pos,
                CEntityEvent {
                    entity_id: self.id(),
                    event: EntityStatus::Poof,
                },
                None,
            );

            world.unregister_player_entity(self);
            world.entity_tracker().on_player_leave(self.id());
            world.player_area_map.remove_by_entity_id(self.id());
            world.chunk_map.remove_player(self);
            self.set_removed(RemovalReason::Killed);
        }
    }

    /// Immediately flushes dirty player entity data to tracking players and self.
    fn sync_entity_data(&self) {
        if let Some(dirty_values) = self.entity_data.lock().pack_dirty() {
            let packet = CSetEntityData::new(self.id(), dirty_values);
            self.get_world()
                .broadcast_to_entity_trackers(self.id(), packet.clone(), None);
            self.send_packet(packet);
        }
    }

    /// Handles a custom payload packet.
    #[expect(clippy::unused_self, reason = "this is an api function")]
    pub fn handle_custom_payload(&self, packet: SCustomPayload) {
        log::info!("Hello from the other side! {packet:?}");
    }

    /// Handles the end of a client tick.
    pub fn handle_client_tick_end(&self) {
        self.movement.lock().finish_client_tick();
    }

    fn push_entities(&self, world: &Arc<World>) {
        if !world.tick_runs_normally() {
            return;
        }

        let pusher = self as &dyn Entity;
        let pushable_entities = world.get_pushable_entities(pusher, &self.bounding_box());
        if pushable_entities.is_empty() {
            return;
        }

        self.apply_entity_cramming_damage(world, &pushable_entities);

        for entity in pushable_entities {
            entity.push_entity(pusher);
        }
    }

    fn apply_entity_cramming_damage(&self, world: &World, pushable_entities: &[SharedEntity]) {
        let max_cramming = world
            .get_game_rule(&MAX_ENTITY_CRAMMING)
            .as_int()
            .unwrap_or(24);

        if max_cramming <= 0 || pushable_entities.len() <= (max_cramming - 1) as usize {
            return;
        }

        let random_roll = self.base.random().lock().next_i32_bounded(4);
        let non_passenger_count = pushable_entities
            .iter()
            .filter(|entity| !entity.is_passenger())
            .count();

        if Self::should_apply_entity_cramming_damage(
            max_cramming,
            pushable_entities.len(),
            non_passenger_count,
            random_roll,
        ) {
            self.hurt(
                &DamageSource::environment(&vanilla_damage_types::CRAMMING),
                6.0,
            );
        }
    }

    const fn should_apply_entity_cramming_damage(
        max_cramming: i32,
        pushable_count: usize,
        non_passenger_count: usize,
        random_roll: i32,
    ) -> bool {
        if max_cramming <= 0 || random_roll != 0 {
            return false;
        }

        let threshold = (max_cramming - 1) as usize;
        pushable_count > threshold && non_passenger_count > threshold
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

        if self.get_health() <= 0.0 {
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

        let Some((took_full_damage, effective_amount)) = self
            .living_base
            .apply_damage_cooldown(amount, source.bypasses_cooldown())
        else {
            return false;
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
                    entity_id: self.id(),
                    source_type_id: type_id,
                    source_cause_id: source.causing_entity_id.map_or(0, |id| id + 1),
                    source_direct_id: source.direct_entity_id.map_or(0, |id| id + 1),
                    source_position: source.source_position,
                },
                None,
            );

            let (yaw, _) = self.rotation();
            world.broadcast_to_nearby(
                chunk_pos,
                CHurtAnimation {
                    entity_id: self.id(),
                    yaw,
                },
                None,
            );
        }

        if self.get_health() <= 0.0 {
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

        self.set_health(self.get_health() - amount);
    }

    /// Vanilla: `ServerPlayer.die()` (does NOT call `super.die()`).
    fn die(&self, source: &DamageSource) {
        if self.is_removed() {
            return;
        }
        if !self.living_base.mark_death_processed() {
            return;
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
                entity_id: self.id(),
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
            player_id: self.id(),
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
        let health = self.get_health();
        if !Self::should_process_respawn(health) {
            return;
        }

        self.living_base.reset_death_state();

        let was_removed = self.base.clear_removed();
        let world = self.get_world();

        // Respawn-specific state: reset health and pose
        {
            let mut entity_data = self.entity_data.lock();
            entity_data
                .living_entity_mut()
                .health
                .set(self.get_max_health());
            entity_data.base_mut().pose.set(EntityPose::Standing);
        }

        // Reset food data to defaults
        *self.food_data.lock() = FoodData::new();

        // Clear transient attribute modifiers (sprint, potion effects, etc.)
        self.attributes().lock().remove_all_transient();

        self.health_sync.lock().reset_for_respawn();

        // TODO: bed/respawn anchor lookup, send NO_RESPAWN_BLOCK_AVAILABLE if missing

        let Some(player_arc) = world.players.get_by_entity_id(self.id()) else {
            return;
        };
        if !was_removed {
            world.unregister_player_entity(self);
        }

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
                && self.game_mode() != GameType::Spectator
            {
                // TODO: drop XP orbs (min(level * 7, 100))
                experience.set_total_points(0);
            }
            // Re-send XP to client after respawn regardless of keepInventory
            experience.dirty = true;
        }

        // TODO: send mob effect packets once effects are implemented
        // TODO: send CInitializeBorder once world border is implemented

        // Shared spawn (teleport, abilities, weather, time, chunk tracking reset)
        let _ = player_arc.spawn(spawn, (0.0, 0.0), ResetReason::Respawn);
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

    /// Vanilla accepts a client respawn request only when player health is dead-or-dying.
    /// Steel's death-processed guard is not respawn authority.
    #[must_use]
    const fn should_process_respawn(health: f32) -> bool {
        health <= 0.0
    }

    /// Returns whether the Player can eat
    pub fn can_eat(&self, can_always_eat: bool) -> bool {
        let invulnerable = { self.abilities.lock().invulnerable };
        let needs_foods = { self.food_data.lock().needs_food() };
        invulnerable || can_always_eat || needs_foods
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
        self.base.set_world(Arc::downgrade(&world));
        self.world.store(world);
    }

    /// Marks the player as switching domains if they are not already in a transition.
    pub fn begin_domain_switch(&self) -> bool {
        self.lifecycle.lock().begin_domain_switch()
    }

    /// Clears the domain-switch transition marker.
    pub fn finish_domain_switch(&self) {
        self.lifecycle.lock().finish_domain_switch();
    }

    /// Returns whether this player is currently switching domains.
    pub fn is_domain_switching(&self) -> bool {
        self.lifecycle.lock().domain_switching()
    }

    /// Returns whether the server has inserted this player into a world.
    #[must_use]
    pub fn has_joined_world(&self) -> bool {
        self.lifecycle.lock().joined_world()
    }

    /// Marks this player as inserted into a world.
    ///
    /// Returns `true` when a client-loaded acknowledgement arrived before world
    /// admission and was applied by this call.
    pub(crate) fn mark_joined_world(&self) -> bool {
        let mut lifecycle = self.lifecycle.lock();
        lifecycle.set_joined_world(true);
        lifecycle.apply_pending_client_loaded()
    }

    /// Returns whether the client has sent its play-loaded signal.
    #[must_use]
    pub fn has_client_loaded(&self) -> bool {
        self.lifecycle.lock().client_loaded()
    }

    /// Marks whether the client has loaded into play.
    pub fn set_client_loaded(&self, client_loaded: bool) {
        self.lifecycle.lock().set_client_loaded(client_loaded);
    }

    /// Applies or buffers the client's play-loaded acknowledgement.
    ///
    /// Returns `true` when the acknowledgement can run gameplay side effects now.
    pub fn mark_client_loaded_from_network(&self) -> bool {
        self.lifecycle.lock().mark_client_loaded_from_network()
    }

    fn tick_client_load_timeout(&self) {
        self.lifecycle.lock().tick_client_load_timeout();
    }

    pub(crate) fn set_pending_root_vehicle(
        &self,
        world: &World,
        root_vehicle: PersistentRootVehicle,
    ) {
        *self.pending_root_vehicle.lock() = Some(PendingRootVehicleRestore {
            world: world.key.clone(),
            root_vehicle,
        });
    }

    pub(crate) fn clear_pending_root_vehicle(&self) {
        *self.pending_root_vehicle.lock() = None;
    }

    pub(crate) fn pending_root_vehicle_for_current_world(&self) -> Option<PersistentRootVehicle> {
        let world_key = self.get_world().key.clone();
        self.pending_root_vehicle
            .lock()
            .as_ref()
            .filter(|pending| pending.world == world_key)
            .map(|pending| pending.root_vehicle.clone())
    }

    pub(crate) fn take_matching_pending_root_vehicle(
        &self,
        world: &World,
        attach: [u8; 16],
        root_uuid: [u8; 16],
    ) -> Option<PersistentRootVehicle> {
        let mut pending = self.pending_root_vehicle.lock();
        let matches = pending.as_ref().is_some_and(|pending| {
            pending.world == world.key
                && pending.root_vehicle.attach == attach
                && pending.root_vehicle.entity.uuid == root_uuid
        });
        if matches {
            pending.take().map(|pending| pending.root_vehicle)
        } else {
            None
        }
    }

    /// Returns this player's local server tick count.
    #[must_use]
    pub fn tick_count(&self) -> i32 {
        self.tick_state.lock().tick_count()
    }

    /// Advances this player's local server tick count.
    fn advance_tick(&self) {
        self.tick_state.lock().advance_tick();
    }

    fn primary_step_sound_block_pos(&self, affecting_pos: BlockPos) -> BlockPos {
        let above_pos = affecting_pos.above();
        let above_state = self.get_world().get_block_state(above_pos);
        let above_block = above_state.get_block();

        if above_block.has_tag(&BlockTag::INSIDE_STEP_SOUND_BLOCKS)
            || above_block.has_tag(&BlockTag::COMBINATION_STEP_SOUND_BLOCKS)
        {
            above_pos
        } else {
            affecting_pos
        }
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
        self.reset_inner_after(new_world, reason, false, || {});
    }

    /// Resets for a domain switch and restores target-domain state after the
    /// player has been detached from the old world's live entity indexes.
    pub(crate) fn reset_after_domain_save_and_restore<F>(
        self: &Arc<Self>,
        new_world: Arc<World>,
        restore_state: F,
    ) where
        F: FnOnce(),
    {
        self.reset_inner_after(new_world, ResetReason::WorldChange, true, restore_state);
    }

    fn reset_inner_after<F>(
        self: &Arc<Self>,
        new_world: Arc<World>,
        reason: ResetReason,
        store_root_vehicle: bool,
        restore_state: F,
    ) where
        F: FnOnce(),
    {
        let old_world = self.get_world();
        let switching_worlds = !Arc::ptr_eq(&old_world, &new_world);

        // --- Old world cleanup (only when actually switching worlds) ---
        if switching_worlds {
            self.do_close_container();
            self.send_packet(CContainerClose { container_id: 0 });
            if store_root_vehicle {
                old_world.remove_player_for_domain_switch(self);
            } else {
                old_world.remove_player_for_world_change(self);
            }
            self.set_world(new_world.clone());
        }

        // --- Reset transient state ---
        self.set_client_loaded(false);
        self.set_velocity(DVec3::ZERO);
        self.movement.lock().reset_last_known_client_movement();
        self.set_on_ground(false);
        self.reset_entity_state();
        *self.block_breaking.lock() = BlockBreakingManager::new();

        // Reset chunk tracking — bump generation counter so the chunk sending tick
        // discards any in-flight batch encoded against the old world.
        {
            let mut chunk_send_epoch = self.chunk_send_epoch.lock();
            *chunk_send_epoch = chunk_send_epoch.wrapping_add(1);
        }
        *self.chunk_sender.lock() = ChunkSender::default();
        *self.last_tracking_view.lock() = None;
        *self.last_chunk_pos.lock() = ChunkPos::new(i32::MAX, i32::MAX);

        restore_state();

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
                gamemode: self.game_mode() as u8,
                previous_gamemode: self.previous_game_mode() as i8,
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
    #[must_use]
    pub fn spawn(
        self: &Arc<Self>,
        position: DVec3,
        rotation: (f32, f32),
        reason: ResetReason,
    ) -> bool {
        let world = self.get_world();

        // Set position and rotation
        self.base.set_position_local(position);
        self.set_rotation(rotation);
        self.set_old_position_to_current();
        self.movement.lock().reset_for_position_sync(position);

        // Teleport sync (sends CPlayerPosition, sets awaiting_teleport for ack)
        if let Err(error) =
            self.teleport(position.x, position.y, position.z, rotation.0, rotation.1)
        {
            panic!(
                "failed to synchronize player {} spawn position: {error}",
                self.id()
            );
        }
        self.reset_flying_ticks();

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
                world.add_player(self.clone(), reason)
            }
            ResetReason::Respawn => {
                // Same world — re-enter chunk tracking
                world.player_area_map.remove_by_entity_id(self.id());
                world.chunk_map.remove_player(self);
                world.entity_tracker().on_player_leave(self.id());

                self.send_packet(CGameEvent {
                    event: GameEventType::LevelChunksLoadStart,
                    data: 0.0,
                });
                world.register_respawned_player_entity(self);
                true
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
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::PLAYER
    }

    fn broadcast_to_player(&self, player: &Player) -> bool {
        if player.is_spectator() {
            true
        } else {
            !self.is_spectator()
        }
    }

    fn tick(&self) {
        // Player tick is handled separately by World::tick_game()
        // This is here for Entity trait compliance
    }

    fn fall_sounds(&self) -> (SoundEventRef, SoundEventRef) {
        (
            &sound_events::ENTITY_PLAYER_SMALL_FALL,
            &sound_events::ENTITY_PLAYER_BIG_FALL,
        )
    }

    fn is_living_entity(&self) -> bool {
        true
    }

    fn is_alive(&self) -> bool {
        !self.is_removed() && self.get_health() > 0.0
    }

    fn forces_fall_flying_velocity_sync(&self) -> bool {
        self.is_fall_flying()
    }

    fn blocks_building(&self) -> bool {
        true
    }

    fn is_pickable(&self) -> bool {
        !self.is_spectator() && !self.is_removed()
    }

    fn is_pushable(&self) -> bool {
        self.get_health() > 0.0 && !self.is_spectator() && !self.on_climbable()
    }

    fn on_climbable(&self) -> bool {
        Player::on_climbable(self)
    }

    fn is_spectator(&self) -> bool {
        self.game_mode() == GameType::Spectator
    }

    fn fire_immune_ticks(&self) -> i32 {
        20
    }

    fn remaining_fire_ticks_cap(&self) -> Option<i32> {
        self.abilities.lock().invulnerable.then_some(1)
    }

    fn get_default_gravity(&self) -> f64 {
        LivingEntity::get_attribute_gravity(self)
    }

    fn fire_ignite_extra_ticks(&self) -> i32 {
        self.get_world().random().lock().next_i32_between(1, 2)
    }

    fn can_freeze(&self) -> bool {
        if self.is_spectator() {
            return false;
        }

        self.default_living_can_freeze()
    }

    fn make_stuck_in_block(&self, state: BlockStateId, speed_multiplier: DVec3) {
        if !self.is_flying() {
            self.default_make_stuck_in_block(state, speed_multiplier);
        }

        // TODO: Reset current impulse context once vehicle/player impulse contexts exist.
    }

    fn can_be_hit_by_projectile(&self) -> bool {
        self.get_health() > 0.0 && self.is_pickable()
    }

    fn uses_client_movement_packets(&self) -> bool {
        true
    }

    fn can_simulate_movement(&self) -> bool {
        true
    }

    fn is_effective_ai(&self) -> bool {
        true
    }

    fn known_movement(&self) -> DVec3 {
        if let Some(vehicle) = self.vehicle()
            && vehicle
                .controlling_passenger()
                .is_none_or(|controller| controller.id() != self.id())
        {
            return vehicle.known_movement();
        }

        self.movement.lock().last_known_client_movement()
    }

    fn known_speed(&self) -> DVec3 {
        if let Some(vehicle) = self.vehicle()
            && vehicle
                .controlling_passenger()
                .is_none_or(|controller| controller.id() != self.id())
        {
            return vehicle.known_speed();
        }

        self.movement.lock().last_known_client_movement()
    }

    fn is_suppressing_bounce(&self) -> bool {
        self.is_crouching()
    }

    fn cause_fall_damage(
        &self,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) -> bool {
        if self.abilities.lock().may_fly {
            return false;
        }

        // TODO: Award `Stats.FALL_ONE_CM` once player statistics are implemented.
        if self.is_fall_damage_immune() {
            return false;
        }

        let attributes = self.attributes().lock();
        let safe_fall_distance = attributes
            .get_value(vanilla_attributes::SAFE_FALL_DISTANCE)
            .unwrap_or(3.0);
        let fall_damage_multiplier = attributes
            .get_value(vanilla_attributes::FALL_DAMAGE_MULTIPLIER)
            .unwrap_or(1.0);
        drop(attributes);

        let damage = LivingEntityBase::calculate_fall_damage(
            fall_distance,
            damage_modifier,
            safe_fall_distance,
            fall_damage_multiplier,
        );
        if damage <= 0 {
            return false;
        }

        self.hurt(source, damage as f32)
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn pack_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
        self.attributes().lock().syncable_snapshots()
    }

    fn drain_dirty_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
        self.attributes().lock().drain_dirty_sync()
    }

    fn max_up_step(&self) -> f32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::STEP_HEIGHT)
            .unwrap_or(0.6) as f32
    }

    fn backs_off_from_edge(&self) -> bool {
        self.is_crouching() && !self.is_flying()
    }

    fn is_pushed_by_fluid(&self) -> bool {
        !self.is_flying()
    }

    fn is_crouching(&self) -> bool {
        Player::is_crouching(self)
    }

    fn can_walk_on_powder_snow(&self) -> bool {
        self.default_living_can_walk_on_powder_snow()
    }

    fn may_interact(&self, world: &World, pos: BlockPos) -> bool {
        world.may_interact(self, pos)
    }

    fn is_swimming(&self) -> bool {
        Player::is_swimming(self)
    }

    fn sound_source(&self) -> SoundSource {
        SoundSource::Players
    }

    fn swim_sound(&self) -> SoundEventRef {
        &sound_events::ENTITY_PLAYER_SWIM
    }

    fn play_step_sound(&self, on_pos: BlockPos, on_state: BlockStateId) {
        if self.is_in_water() {
            self.water_swim_sound();
            self.play_muffled_step_sound(on_state);
            return;
        }

        let primary_step_sound_pos = self.primary_step_sound_block_pos(on_pos);
        if primary_step_sound_pos == on_pos {
            self.play_block_step_sound(on_state);
        } else {
            let primary_state = self.get_world().get_block_state(primary_step_sound_pos);
            if primary_state
                .get_block()
                .has_tag(&BlockTag::COMBINATION_STEP_SOUND_BLOCKS)
            {
                self.play_combination_step_sounds(primary_state, on_state);
            } else {
                self.play_block_step_sound(primary_state);
            }
        }
    }

    fn on_below_world(&self) {
        self.hurt(
            &DamageSource::environment(&vanilla_damage_types::OUT_OF_WORLD),
            4.0,
        );
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
            if let Err(error) = self.teleport(pos.x, pos.y, pos.z, rotation.0, rotation.1) {
                panic!(
                    "failed to commit same-world portal teleport for player {}: {error}",
                    self.id()
                );
            }
            self.reset_flying_ticks();
        } else {
            self.reset(new_world, ResetReason::WorldChange);
            // TODO: set portal cooldown from teleport_transition.portal_cooldown
            if !self.spawn(
                teleport_transition.position,
                teleport_transition.rotation,
                ResetReason::WorldChange,
            ) {
                return;
            }
            // Vanilla: PlayerList.sendAllPlayerInfo -> inventoryMenu.sendAllDataToRemote
            self.send_inventory_to_remote();
        }
    }
}

impl LivingEntity for Player {
    fn get_health(&self) -> f32 {
        *self.entity_data.lock().living_entity().health.get()
    }

    fn set_health(&self, health: f32) {
        let max_health = self.get_max_health();
        let clamped = health.clamp(0.0, max_health);
        self.entity_data
            .lock()
            .living_entity_mut()
            .health
            .set(clamped);
    }

    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
    }

    fn with_equipment_slot(&self, slot: EquipmentSlot, visitor: &mut dyn FnMut(&ItemStack)) {
        let inventory = self.inventory.lock();
        visitor(inventory.equipment().get_ref(slot));
    }

    fn with_equipment_slot_mut(
        &self,
        slot: EquipmentSlot,
        visitor: &mut dyn FnMut(&mut ItemStack),
    ) {
        let mut inventory = self.inventory.lock();
        visitor(inventory.equipment_mut().get_mut(slot));
    }

    fn has_infinite_materials(&self) -> bool {
        Player::has_infinite_materials(self)
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

    fn is_affected_by_fluids(&self) -> bool {
        !self.is_flying()
    }

    fn can_glide(&self) -> bool {
        !self.is_flying() && self.default_can_glide()
    }

    fn is_immobile(&self) -> bool {
        self.default_is_immobile() || self.is_sleeping()
    }

    fn jump_from_ground(&self) {
        self.default_jump_from_ground();
        // TODO: Award Stats.JUMP once player statistics exist.
        if self.is_sprinting() {
            self.cause_food_exhaustion(0.2);
        } else {
            self.cause_food_exhaustion(0.05);
        }
    }

    fn ai_step(&self) -> Option<MoveResult> {
        if self.is_flying() && !self.is_passenger() {
            self.reset_fall_distance();
        }

        self.default_ai_step()
    }

    fn travel(&self, input: DVec3) -> Option<MoveResult> {
        if self.is_passenger() {
            return self.default_travel(input);
        }

        if self.is_swimming() {
            let look_angle_y = self.look_angle().y;
            let multiplier = if look_angle_y < -0.2 { 0.085 } else { 0.06 };
            let has_fluid_above = self.level().is_some_and(|world| {
                let position = self.position();
                let pos = BlockPos::containing(position.x, position.y + 0.9, position.z);
                !get_fluid_state(&world, pos).is_empty()
            });
            if look_angle_y <= 0.0 || self.is_jumping() || has_fluid_above {
                let velocity = self.velocity();
                self.set_velocity(
                    velocity + DVec3::new(0.0, (look_angle_y - velocity.y) * multiplier, 0.0),
                );
            }
        }

        if self.is_flying() {
            let original_movement_y = self.velocity().y;
            let result = self.default_travel(input);
            let velocity = self.velocity();
            self.set_velocity(DVec3::new(
                velocity.x,
                original_movement_y * 0.6,
                velocity.z,
            ));
            result
        } else {
            self.default_travel(input)
        }
    }

    fn get_flying_speed(&self) -> f32 {
        if self.is_flying() && !self.is_passenger() {
            let flying_speed = self.abilities.lock().flying_speed;
            if self.is_sprinting() {
                flying_speed * 2.0
            } else {
                flying_speed
            }
        } else if self.is_sprinting() {
            0.025_999_999
        } else {
            0.02
        }
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

#[cfg(test)]
mod tests {
    use super::Player;

    #[test]
    fn respawn_request_is_allowed_after_dead_reconnect() {
        assert!(Player::should_process_respawn(0.0));
    }

    #[test]
    fn respawn_request_is_ignored_while_alive() {
        assert!(!Player::should_process_respawn(20.0));
    }

    #[test]
    fn respawn_request_uses_health_not_death_processed_guard() {
        struct RespawnGateInput {
            health: f32,
            death_processed: bool,
        }

        let input = RespawnGateInput {
            health: 20.0,
            death_processed: true,
        };

        assert!(input.death_processed);
        assert!(!Player::should_process_respawn(input.health));
    }

    #[test]
    fn entity_cramming_requires_random_zero_and_threshold_overflow() {
        assert!(Player::should_apply_entity_cramming_damage(24, 24, 24, 0));
        assert!(!Player::should_apply_entity_cramming_damage(24, 24, 24, 1));
        assert!(!Player::should_apply_entity_cramming_damage(24, 23, 24, 0));
    }

    #[test]
    fn entity_cramming_counts_only_non_passengers_for_damage() {
        assert!(!Player::should_apply_entity_cramming_damage(24, 24, 23, 0));
    }

    #[test]
    fn entity_cramming_disabled_when_gamerule_is_zero() {
        assert!(!Player::should_apply_entity_cramming_damage(0, 100, 100, 0));
    }
}
