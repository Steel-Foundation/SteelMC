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
/// The vanilla `ServerPlayer` split: the outer per-connection session that owns
/// network/session state and references the locked [`Player`] entity.
pub mod server_player;
mod signature_cache;
mod spam_throttler;
mod teleport_state;
mod tick_state;
pub mod view;

pub use abilities::Abilities;
use container_counter::ContainerCounter;
use food_data::FoodData;
use glam::DVec3;
use health_sync::HealthSyncState;
pub use input_state::PlayerInput;
pub use message_validator::LastSeenMessagesValidator;
use movement_state::MovementState;
pub use server_player::ServerPlayer;
pub use signature_cache::{LastSeen, MessageCache};
use steel_protocol::{
    packet_traits::{CompressionInfo, EncodedPacket},
    packets::game::{CSetEntityData, CSetExperience},
};
use teleport_state::TeleportState;
use tick_state::PlayerTickState;
use view::PlayerView;

use block_breaking::BlockBreakingManager;
use enum_dispatch::enum_dispatch;
use game_mode_state::PlayerGameModeState;
pub use game_profile::{GameProfile, GameProfileAction};
use std::cell::RefCell;
use std::sync::{Arc, Weak};
use steel_protocol::packets::game::{
    AttributeSnapshot, CEntityEvent, CPlayerCombatKill, CRespawn, CSetHealth, CSetHeldSlot,
    CSetPassengers, CSetTime, CUpdateAttributes, ClientCommandAction, EquipmentSlotItem,
    SoundSource,
};
use steel_registry::RegistryEntry;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_data::EntityPose;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::game_rules::GameRuleValue;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entity_data::PlayerEntityData;
use steel_registry::vanilla_game_rules::{
    ADVANCE_TIME, IMMEDIATE_RESPAWN, KEEP_INVENTORY, MAX_ENTITY_CRAMMING, SHOW_DEATH_MESSAGES,
};
use steel_registry::{sound_events, vanilla_attributes, vanilla_entities, vanilla_particle_types};
use steel_utils::entity_events::EntityStatus;

use steel_utils::locks::SyncMutex;
use steel_utils::random::Random as _;
use steel_utils::types::{Difficulty, GameType};
use text_components::TextComponent;
use text_components::resolving::TextResolutor;
use text_components::translation::TranslatedMessage;
use text_components::{content::Resolvable, custom::CustomData};

use crate::config::RuntimeConfig;
use crate::enchantment_helper;
use crate::entity::damage::DamageSource;
use crate::entity::{
    DEATH_DURATION, Entity, EntityBase, EntityEventSource, EntitySyncedData, LivingEntity,
    LivingEntityBase, MobEffectSyncChange, MobEffectSyncPacket, RemovalReason, SharedEntity,
    equipment_items_to_packet_items, start_riding_entities,
};
use crate::fluid::get_fluid_state;
use crate::inventory::{SyncPlayerInv, equipment::EquipmentSlot};
use crate::physics::MoveResult;
use crate::player::experience::Experience;
use crate::player::player_data::PersistentRootVehicle;
use crate::player::player_inventory::PlayerInventory;
use crate::server::{PlayerResetRequest, Server};
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

use crate::player::chunk_sender::ChunkSender;
use crate::player::networking::JavaConnection;
use crate::portal::TeleportTransition;
use crate::world::World;

/// Shared handle to a player.
///
/// Like every other entity, a player is reached mutably by locking; the game
/// tick and all cross-entity mutation lock this to obtain `&mut Player` and call
/// the `&mut self` `Entity`/`LivingEntity` methods. Held by the player map, the
/// connection, the inventory back-reference, and `EntityBase`.
pub type SharedPlayer = Arc<SyncMutex<Player>>;

/// A struct representing a player.
pub struct Player {
    /// The player's game profile.
    pub gameprofile: GameProfile,

    /// Back reference to the outer [`ServerPlayer`] session (connection, chat,
    /// chunk sender, view, world, server/config, …). Upgraded to reach network
    /// and session state without re-locking the entity. Held weakly because the
    /// `ServerPlayer` owns the entity `Arc`.
    server_player: Weak<ServerPlayer>,

    /// Common entity fields (id, uuid, position, rotation, removal, callback).
    base: Arc<EntityBase>,
    /// Downgraded copy of `base` for the `Entity::base_weak` accessor.
    base_weak: Weak<EntityBase>,

    /// Movement tracking state
    pub(crate) movement: MovementState,

    /// Synchronized entity data (health, pose, flags, etc.) for network sync.
    entity_data: PlayerEntityData,

    /// Current and previous game mode.
    game_modes: PlayerGameModeState,

    /// The player's inventory container (shared with `inventory_menu`).
    pub inventory: SyncPlayerInv,

    /// Last main-hand stack used for vanilla attack-strength reset checks.
    last_item_in_main_hand: ItemStack,

    /// The player's inventory menu (always open, even when `container_id` is 0).
    inventory_menu: RefCell<InventoryMenu>,

    /// The currently open menu (None if player inventory is open).
    /// This is separate from `inventory_menu` which is always present.
    open_menu: RefCell<Option<Box<dyn MenuInstance>>>,

    /// Counter for generating container IDs (1-100, wraps around).
    container_counter: ContainerCounter,

    /// Player abilities (flight, invulnerability, build permissions, speeds, etc.)
    pub abilities: Abilities,

    /// Block breaking state machine.
    pub block_breaking: BlockBreakingManager,

    /// Shared living-entity runtime fields (attributes, speed, damage/death state).
    /// Vanilla: `LivingEntity` (L230-232) + `Entity.invulnerableTime` (L256).
    living_base: LivingEntityBase,

    /// Player food/hunger state (food level, saturation, exhaustion).
    pub food_data: FoodData,

    /// Delta-tracking state for `CSetHealth` deduplication.
    health_sync: HealthSyncState,

    /// The Player's Experience
    pub experience: Experience,

    /// Persisted `RootVehicle` payload awaiting live entity restoration.
    pending_root_vehicle: Option<PendingRootVehicleRestore>,
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
            .get_value(vanilla_attributes::BLOCK_INTERACTION_RANGE)
            .unwrap_or(4.5);
        let direction = self.look_angle() * block_interaction_range;

        let end_pos = start_pos + direction;
        (start_pos, end_pos)
    }

    /// Returns this player's entity handle.
    ///
    /// Use this wherever a player must flow into entity-typed storage or
    /// parameters (goal targets, passenger lists, trackers).
    #[must_use]
    pub fn shared_entity(&self) -> SharedEntity {
        self.base.clone()
    }

    /// Returns the player's current game mode.
    #[must_use]
    pub fn game_mode(&self) -> GameType {
        self.game_modes.current()
    }

    /// Returns the player's previous game mode.
    #[must_use]
    pub fn previous_game_mode(&self) -> Option<GameType> {
        self.game_modes.previous()
    }

    /// Restores current and previous game mode from persistent player data.
    pub(crate) fn restore_game_modes(&mut self, current: GameType, previous: Option<GameType>) {
        self.game_modes.set_pair(current, previous);
    }

    /// Changes the current game mode and records the old current mode as previous.
    fn change_game_mode_state(&mut self, game_mode: GameType) -> bool {
        self.game_modes.change_current(game_mode)
    }

    /// Creates a new player entity.
    ///
    /// Called from [`ServerPlayer::new`] inside `Arc::new_cyclic`. `entity` is the
    /// entity's own weak handle (for the entity base back-ref and inventory), and
    /// `server_player` is the weak back-ref to the owning session. `world` is only
    /// used to seed the entity base's world weak; the live world lives on the
    /// [`ServerPlayer`].
    pub fn new(
        gameprofile: GameProfile,
        world: &Arc<World>,
        entity_id: i32,
        entity: &Weak<SyncMutex<Player>>,
        server_player: Weak<ServerPlayer>,
    ) -> Self {
        // Create a single shared inventory container used by both the player and inventory menu
        let inventory = Arc::new(SyncMutex::new(PlayerInventory::new()));

        let pos = DVec3::new(0.0, 0.0, 0.0);

        let living_base = LivingEntityBase::new(&vanilla_entities::PLAYER);
        let player_uuid = gameprofile.id;
        let world_ref = Arc::downgrade(world);

        let base = {
            let mut base = EntityBase::with_uuid(
                entity_id,
                player_uuid,
                pos,
                Self::dimensions_for_pose(EntityPose::Standing),
                world_ref,
            );
            base.attach_player(entity.clone());
            Arc::new(base)
        };
        let base_weak = Arc::downgrade(&base);

        Self {
            gameprofile,
            server_player,
            base,
            base_weak,
            movement: MovementState::new(),
            entity_data: {
                let mut data = PlayerEntityData::new();
                living_base.initialize_synced_data(&mut data);
                data
            },
            game_modes: PlayerGameModeState::new(GameType::Survival),
            inventory: inventory.clone(),
            last_item_in_main_hand: ItemStack::empty(),
            inventory_menu: RefCell::new(InventoryMenu::new(inventory)),
            open_menu: RefCell::new(None),
            container_counter: ContainerCounter::new(),
            abilities: Abilities::default(),
            block_breaking: BlockBreakingManager::new(),
            living_base,
            food_data: FoodData::new(),
            health_sync: HealthSyncState::new(),
            experience: Experience::default(),
            pending_root_vehicle: None,
        }
    }

    /// Returns the owning [`ServerPlayer`] session handle.
    ///
    /// # Panics
    /// Panics if the session has been dropped (the entity must not outlive it).
    #[must_use]
    pub fn server_player(&self) -> Arc<ServerPlayer> {
        self.server_player
            .upgrade()
            .expect("player entity must not outlive its ServerPlayer session")
    }

    /// Returns the player's connection handle (cheap `Arc` clone).
    #[must_use]
    pub fn connection(&self) -> Arc<PlayerConnection> {
        Arc::clone(&self.server_player().connection)
    }

    /// Returns the player's lock-free published view handle.
    #[must_use]
    pub fn view(&self) -> Arc<PlayerView> {
        Arc::clone(&self.server_player().view)
    }

    /// Returns the player's chunk sender handle.
    #[must_use]
    pub fn chunk_sender(&self) -> Arc<SyncMutex<ChunkSender>> {
        Arc::clone(&self.server_player().chunk_sender)
    }

    /// Repositions this riding player onto its vehicle's seat.
    ///
    /// Mirrors vanilla `Entity.positionRider` (which Steel would run from
    /// `ride_tick`, a no-op for players). The seat world-position is read under
    /// the vehicle's behavior lock, but the position commit runs *after* that
    /// lock is released: [`try_set_position`](Entity::try_set_position) triggers
    /// the player's move-committed tracker refresh, which re-locks nearby tracked
    /// entities — including this very vehicle — and would self-deadlock if the
    /// vehicle lock were still held.
    fn follow_vehicle(&mut self) {
        let Some(vehicle) = self.vehicle() else {
            return;
        };
        let riding_position = vehicle.with_entity(|v| {
            if v.has_passenger(self.as_entity_event_source()) {
                Some(v.passenger_riding_position(self.as_entity_event_source()))
            } else {
                None
            }
        });
        let Some(riding_position) = riding_position else {
            return;
        };
        let vehicle_attachment = self.vehicle_attachment_point(self.as_entity_event_source());
        if let Err(error) = self.try_set_position(riding_position - vehicle_attachment) {
            log::debug!("Failed to position riding player {}: {error}", self.id());
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
    pub fn tick(&mut self) {
        // Vanilla `Player.rideTick`: Steel never ride-ticks players (they tick
        // outside the entity manager, and their `EntityBase` entity cell is empty
        // so `ride_tick_entity` no-ops), so the passenger-tick work lives here.
        // A sneaking passenger dismounts; otherwise the player follows its vehicle
        // server-side. Without the reposition the player's server position stays
        // frozen at the mount point for the whole ride (client rides fine, but the
        // server sees the player where they boarded — breaks summon, tracking, etc).
        if self.is_passenger() {
            if self.wants_to_stop_riding() {
                Entity::stop_riding(self);
            } else {
                self.follow_vehicle();
            }
        }

        self.flush_equipment_attribute_refreshes();
        self.advance_tick();
        self.tick_attack_strength();
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
        self.movement.reset_for_tick(tick_position);
        self.set_old_position_to_current();
        self.reset_vehicle_movement_for_tick();

        self.default_tick();
        let world = self.get_world();
        self.apply_world_border_damage(&world);
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
        self.tick_mob_effects();

        if self.get_health() <= 0.0 {
            self.tick_death();
        } else {
            self.touch_nearby_items();
            let mut block_breaking = std::mem::take(&mut self.block_breaking);
            block_breaking.tick(self, &world);
            self.block_breaking = block_breaking;
            self.apply_effects_from_blocks();
            self.push_entities(&world);

            // TODO: Implement remaining player ticking logic here
            // - Managing game mode specific logic
            // - Updating advancements
            // - Handling falling

            self.update_player_attributes();
            self.living_base.refresh_speed_from_attributes();
            self.tick_regeneration();

            if self.is_sprinting() && !self.food_data.has_enough_food() {
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
                let food_data = &self.food_data;
                (food_data.food_level, food_data.saturation_level)
            };

            let saturation_zero = saturation == 0.0;

            if self.health_sync.needs_update(health, food, saturation_zero) {
                self.send_packet(CSetHealth {
                    health,
                    food,
                    food_saturation: saturation,
                });
                self.health_sync.record_sent(health, food, saturation_zero);
            }
        }

        if self.experience.dirty {
            self.send_packet(CSetExperience {
                progress: self.experience.progress() as f32,
                level: self.experience.level(),
                total_experience: self.experience.total_points(),
            });
            self.experience.dirty = false;
        }

        // Sync the player's own dirty attributes (e.g. ATTACK_SPEED from held
        // equipment) to nearby players and the player itself. The entity tracker
        // skips players entirely (`EntityBase::get_entity` is `None` for them,
        // since players are stored in the separate `player` field), so without
        // this the client never learns the held weapon's attack speed — breaking
        // the attack-strength crosshair indicator. Vanilla: `ServerEntity` sends
        // attribute updates via `broadcastAndSend`.
        let attribute_snapshots = self.attributes_mut().drain_dirty_sync();
        if !attribute_snapshots.is_empty() {
            let chunk_pos = self.view().last_chunk_pos();
            self.get_world().broadcast_to_nearby(
                chunk_pos,
                CUpdateAttributes::new(self.id(), attribute_snapshots),
                None,
            );
        }

        self.connection().tick();
    }

    fn refresh_equipment_attribute_modifiers_from_stack(
        &mut self,
        slot: EquipmentSlot,
        item_stack: &ItemStack,
    ) {
        self.living_base
            .refresh_equipment_attribute_modifiers(slot, item_stack);
    }

    /// Applies any equipment attribute-modifier refreshes queued by inventory
    /// mutations. Runs with the player lock held so it reaches `living_base`
    /// directly, instead of the inventory re-locking the player (which would
    /// deadlock under the tick).
    fn flush_equipment_attribute_refreshes(&mut self) {
        let pending = self.inventory.lock().drain_pending_attribute_refreshes();
        for (slot, item) in pending {
            self.refresh_equipment_attribute_modifiers_from_stack(slot, &item);
        }
    }

    /// Ticks the death animation timer.
    /// Vanilla: `LivingEntity.tickDeath()` (not overridden by `ServerPlayer`).
    fn tick_death(&mut self) {
        let death_time = self.living_base.increment_death_time();

        if death_time >= DEATH_DURATION && !self.is_removed() {
            let world = self.get_world();
            let chunk_pos = self.view().last_chunk_pos();
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
            world.chunk_map.remove_player(&self.server_player());
            self.set_removed(RemovalReason::Killed);
        }
    }

    /// Immediately flushes dirty player entity data to tracking players and self.
    fn sync_entity_data(&self) {
        if let Some(dirty_values) = self.entity_data.pack_dirty() {
            let packet = CSetEntityData::new(self.id(), dirty_values);
            self.get_world()
                .broadcast_to_entity_trackers(self.id(), packet.clone(), None);
            self.send_packet(packet);
        }
    }

    fn update_dirty_mob_effect_entity_data(&mut self) {
        if !self.living_base.take_effects_dirty() {
            return;
        }

        let Some(particle_type_id) = vanilla_particle_types::ENTITY_EFFECT.try_id() else {
            log::error!("vanilla entity_effect particle type is not registered");
            return;
        };
        let Ok(particle_type_id) = i32::try_from(particle_type_id) else {
            log::error!("vanilla entity_effect particle type id does not fit protocol i32");
            return;
        };
        let display = self.living_base.mob_effect_display_state(particle_type_id);

        {
            let entity_data = &mut self.entity_data;
            let living = entity_data.living_entity_mut();
            living.effect_particles.set(display.particles);
            living.effect_ambience.set(display.ambient);
        }

        self.entity_data.set_base_invisible_flag(display.invisible);
        self.entity_data
            .set_base_glowing_flag(self.has_glowing_tag() || display.glowing);
    }

    /// Handles a custom payload packet.
    #[expect(clippy::unused_self, reason = "this is an api function")]
    pub fn handle_custom_payload(&self, packet: SCustomPayload) {
        log::info!("Hello from the other side! {packet:?}");
    }

    /// Handles the end of a client tick.
    pub fn handle_client_tick_end(&mut self) {
        self.movement.finish_client_tick();
    }

    fn push_entities(&mut self, world: &Arc<World>) {
        if !world.tick_runs_normally() {
            return;
        }

        let pushable_entities =
            world.get_pushable_entities(self.shared_entity(), &self.bounding_box());
        if pushable_entities.is_empty() {
            return;
        }

        self.apply_entity_cramming_damage(world, &pushable_entities);

        for entity in pushable_entities {
            entity.push_entity(self);
        }
    }

    fn apply_entity_cramming_damage(&mut self, world: &World, pushable_entities: &[SharedEntity]) {
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

    fn apply_world_border_damage(&mut self, world: &World) {
        if !world.tick_runs_normally() || self.get_health() <= 0.0 {
            return;
        }

        let border = world.world_border_snapshot();
        let position = self.position();
        let Some(damage) =
            border.outside_damage_amount(position.x, position.z, self.bounding_box())
        else {
            return;
        };

        self.hurt(
            &DamageSource::environment(&vanilla_damage_types::OUTSIDE_BORDER),
            damage,
        );
    }

    /// Main entry point for dealing damage. Returns `true` if damage was applied.
    pub fn hurt(&mut self, source: &DamageSource, amount: f32) -> bool {
        if LivingEntity::is_invulnerable_to(self, source) {
            return false;
        }

        {
            if self.abilities.invulnerable && !source.bypasses_invulnerability() {
                return false;
            }
        }

        // TODO: reset player noActionTime and remove shoulder entities.
        if self.get_health() <= 0.0 {
            return false;
        }

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

        if amount == 0.0 {
            return false;
        }

        LivingEntity::hurt_server(self, source, amount)
    }

    /// Applies damage after reductions.
    /// TODO: armor, enchantment, absorption
    fn actually_hurt(&mut self, source: &DamageSource, amount: f32) {
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
    fn die(&mut self, source: &DamageSource) {
        if self.is_removed() {
            return;
        }
        if !self.living_base.mark_death_processed() {
            return;
        }

        {
            let experience = &mut self.experience;

            experience.sync_score(&mut self.entity_data);
            experience.score = 0;
        }

        self.sync_entity_data();

        // NOTE: Vanilla `ServerPlayer.die()` does NOT set Pose::Dying — only
        // `LivingEntity.die()` does (which ServerPlayer never calls via super).
        // The death screen covers the player model, so the pose is irrelevant.

        let world = self.get_world();

        // Broadcast entity event 3 (death sound) to all nearby players.
        let chunk_pos = self.view().last_chunk_pos();
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

        self.clear_fire();
        self.set_ticks_frozen(0);

        if world.get_game_rule(&IMMEDIATE_RESPAWN) == GameRuleValue::Bool(true) {
            self.respawn();
        }
    }

    /// TODO: bed/respawn anchor, cross-world, noRespawnBlockAvailable
    ///
    /// # Panics
    /// If the player dies in a world that doesn't exist.
    pub fn respawn(&mut self) {
        let health = self.get_health();
        if !Self::should_process_respawn(health) {
            return;
        }

        let world = self.get_world();
        self.reset_state_for_death_respawn();
        let was_removed = self.base.clear_removed();

        // TODO: bed/respawn anchor lookup, send NO_RESPAWN_BLOCK_AVAILABLE if missing

        if world.players.get_by_entity_id(self.id()).is_none() {
            return;
        }
        if !was_removed {
            world.unregister_player_entity(self);
        }

        // The reset+spawn tail re-locks this entity (currently locked by the tick),
        // so defer it to a safe point. See `ServerPlayer::finish_respawn`.
        self.server()
            .queue_player_reset(PlayerResetRequest::Respawn(self.server_player()));
    }

    fn reset_state_for_death_respawn(&mut self) {
        self.close_container();
        self.detach_relationships_for_respawn();

        self.attributes_mut().remove_all_transient();
        self.living_base = LivingEntityBase::new(&vanilla_entities::PLAYER);
        self.base
            .reset_for_player_respawn(Self::dimensions_for_pose(EntityPose::Standing));

        self.set_health(self.get_max_health());
        self.set_pose(EntityPose::Standing);
        self.reset_entity_state();
        self.sync_base_entity_data();
        self.update_dirty_mob_effect_entity_data();

        self.food_data = FoodData::new();
        self.block_breaking = BlockBreakingManager::new();
        *self.server_player().teleport_state.lock() = TeleportState::new();
        *self.server_player().tick_state.lock() = PlayerTickState::new();
        self.last_item_in_main_hand = ItemStack::empty();
        self.health_sync.reset_for_respawn();
        self.clear_pending_root_vehicle();
        self.movement.reset_last_known_client_movement();
    }

    fn detach_relationships_for_respawn(&self) {
        for passenger in self.passengers() {
            passenger.stop_riding();
        }
        self.stop_riding();
        self.base.set_boarding_cooldown(0);
    }

    /// Handles client commands, requestStats and `RequestGameRuleValues` are still todo
    pub fn handle_client_command(&mut self, action: ClientCommandAction) {
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
        let invulnerable = self.abilities.invulnerable;
        let needs_foods = self.food_data.needs_food();
        invulnerable || can_always_eat || needs_foods
    }

    /// Cleans up player resources.
    #[expect(clippy::unused_self, reason = "this is an api function")]
    pub const fn cleanup(&self) {}

    /// Returns the world the player is currently in.
    pub fn get_world(&self) -> Arc<World> {
        self.server_player().world.load_full()
    }

    /// Returns the server this player belongs to.
    pub(crate) fn server(&self) -> Arc<Server> {
        self.server_player()
            .server
            .upgrade()
            .expect("player must not outlive server")
    }

    /// Returns the runtime configuration shared with the server.
    pub(crate) fn config(&self) -> Arc<RuntimeConfig> {
        Arc::clone(&self.server_player().config)
    }

    /// Sets the world the player is in.
    ///
    /// This is used when the correct world isn't known at construction time
    /// (e.g., when loading saved player data determines the actual world).
    pub fn set_world(&self, world: Arc<World>) {
        self.base.set_world(Arc::downgrade(&world));
        self.server_player().world.store(world);
    }

    /// Marks the player as switching domains if they are not already in a transition.
    pub fn begin_domain_switch(&self) -> bool {
        self.server_player().lifecycle.lock().begin_domain_switch()
    }

    /// Clears the domain-switch transition marker.
    pub fn finish_domain_switch(&self) {
        self.server_player().lifecycle.lock().finish_domain_switch();
    }

    /// Returns whether this player is currently switching domains.
    pub fn is_domain_switching(&self) -> bool {
        self.server_player().lifecycle.lock().domain_switching()
    }

    /// Returns whether the server has inserted this player into a world.
    #[must_use]
    pub fn has_joined_world(&self) -> bool {
        self.server_player().lifecycle.lock().joined_world()
    }

    /// Marks this player as inserted into a world.
    ///
    /// Returns `true` when a client-loaded acknowledgement arrived before world
    /// admission and was applied by this call.
    pub(crate) fn mark_joined_world(&self) -> bool {
        let sp = self.server_player();
        let mut lifecycle = sp.lifecycle.lock();
        lifecycle.set_joined_world(true);
        lifecycle.apply_pending_client_loaded()
    }

    /// Returns whether the client has sent its play-loaded signal.
    #[must_use]
    pub fn has_client_loaded(&self) -> bool {
        self.server_player().lifecycle.lock().client_loaded()
    }

    /// Marks whether the client has loaded into play.
    pub fn set_client_loaded(&self, client_loaded: bool) {
        self.server_player()
            .lifecycle
            .lock()
            .set_client_loaded(client_loaded);
    }

    /// Applies or buffers the client's play-loaded acknowledgement.
    ///
    /// Returns `true` when the acknowledgement can run gameplay side effects now.
    pub fn mark_client_loaded_from_network(&self) -> bool {
        self.server_player()
            .lifecycle
            .lock()
            .mark_client_loaded_from_network()
    }

    fn tick_client_load_timeout(&self) {
        self.server_player()
            .lifecycle
            .lock()
            .tick_client_load_timeout();
    }

    pub(crate) fn set_pending_root_vehicle(
        &mut self,
        world: &World,
        root_vehicle: PersistentRootVehicle,
    ) {
        self.pending_root_vehicle = Some(PendingRootVehicleRestore {
            world: world.key.clone(),
            root_vehicle,
        });
    }

    pub(crate) fn clear_pending_root_vehicle(&mut self) {
        self.pending_root_vehicle = None;
    }

    pub(crate) fn pending_root_vehicle_for_current_world(&self) -> Option<PersistentRootVehicle> {
        let world_key = self.get_world().key.clone();
        self.pending_root_vehicle
            .as_ref()
            .filter(|pending| pending.world == world_key)
            .map(|pending| pending.root_vehicle.clone())
    }

    pub(crate) fn take_matching_pending_root_vehicle(
        &mut self,
        world: &World,
        attach: [u8; 16],
        root_uuid: [u8; 16],
    ) -> Option<PersistentRootVehicle> {
        let pending = &mut self.pending_root_vehicle;
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
        self.server_player().tick_state.lock().tick_count()
    }

    /// Returns vanilla `Player.takeXpDelay`.
    #[must_use]
    pub(crate) fn take_xp_delay(&self) -> i32 {
        self.server_player().tick_state.lock().take_xp_delay()
    }

    /// Sets vanilla `Player.takeXpDelay`.
    pub(crate) fn set_take_xp_delay(&self, delay: i32) {
        self.server_player()
            .tick_state
            .lock()
            .set_take_xp_delay(delay);
    }

    /// Gives raw experience points to this player.
    pub(crate) fn give_experience_points(&mut self, points: i32) {
        self.experience.add_points(points);
    }

    /// Advances this player's local server tick count.
    fn advance_tick(&self) {
        self.server_player().tick_state.lock().advance_tick();
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

    fn passenger_ids_for_packet(entity: &dyn Entity) -> Vec<i32> {
        entity
            .passengers()
            .iter()
            .map(|passenger| passenger.id())
            .collect()
    }

    fn send_mob_effect_sync_packet(&self, packet: MobEffectSyncPacket) {
        match packet {
            MobEffectSyncPacket::Update(packet) => self.send_packet(packet),
            MobEffectSyncPacket::Remove(packet) => self.send_packet(packet),
        }
    }

    fn send_active_effects_for_vehicle(&self, vehicle: &dyn Entity) {
        let Some(living_vehicle) = vehicle.as_living_entity() else {
            return;
        };
        for effect in living_vehicle.active_mob_effects() {
            self.send_mob_effect_sync_packet(
                MobEffectSyncChange::Update {
                    effect,
                    blend_for_self: false,
                }
                .packet(vehicle.id(), false),
            );
        }
    }

    fn remove_active_effects_for_vehicle(&self, vehicle: &dyn Entity) {
        let Some(living_vehicle) = vehicle.as_living_entity() else {
            return;
        };
        for effect in living_vehicle.active_mob_effects() {
            self.send_mob_effect_sync_packet(
                MobEffectSyncChange::Remove {
                    effect: effect.effect(),
                }
                .packet(vehicle.id(), false),
            );
        }
    }
}

impl ServerPlayer {
    /// Drains and applies all queued inbound packets on the game tick.
    ///
    /// The connection listener enqueues game-state packets off the IO task; this
    /// applies them here so player state is mutated by a single thread. Called at
    /// the start of the player's tick.
    pub fn drain_inbound(self: &Arc<Self>) {
        let Some(server) = self.server.upgrade() else {
            return;
        };
        // Drain one packet at a time, never holding the entity lock across
        // `apply_inbound_packet` (which locks the entity itself).
        loop {
            let packet = {
                let mut rx = self.inbound_rx.lock();
                match rx.try_recv() {
                    Ok(packet) => packet,
                    Err(_) => break,
                }
            };
            if let Err(err) = JavaConnection::apply_inbound_packet(self, packet, &server) {
                log::warn!(
                    "Failed to apply inbound packet for player {}: {err}",
                    self.entity_base.id()
                );
            }
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
        let old_world = self.entity.lock().get_world();
        let switching_worlds = !Arc::ptr_eq(&old_world, &new_world);

        if switching_worlds {
            {
                let player = self.entity.lock();
                player.do_close_container();
                player.send_packet(CContainerClose { container_id: 0 });
            }
            // These re-lock the entity internally, so the guard must be dropped first.
            if store_root_vehicle {
                old_world.remove_player_for_domain_switch(self);
            } else {
                old_world.remove_player_for_world_change(self);
            }
            self.entity.lock().set_world(new_world.clone());
        }

        {
            let mut player = self.entity.lock();
            player.set_client_loaded(false);
            player.set_velocity(DVec3::ZERO);
            player.movement.reset_last_known_client_movement();
            player.set_on_ground(false);
            player.reset_entity_state();
            player.block_breaking = BlockBreakingManager::new();
        }

        // Reset chunk tracking — bump generation counter so the chunk sending tick
        // discards any in-flight batch encoded against the old world. These live on
        // the session, so no entity lock is needed.
        self.view.bump_chunk_send_epoch();
        *self.chunk_sender.lock() = ChunkSender::default();
        *self.last_tracking_view.lock() = None;
        self.view
            .set_last_chunk_pos(ChunkPos::new(i32::MAX, i32::MAX));

        // Closure may re-lock the entity (domain-state restore), so run it unlocked.
        restore_state();

        if reason != ResetReason::InitialJoin {
            // 0x01 = keep attributes, 0x02 = keep entity data
            let data_kept: i8 = match reason {
                ResetReason::WorldChange => 0x03,
                _ => 0x00,
            };

            let player = self.entity.lock();
            player.send_packet(CRespawn {
                dimension_type: new_world.dimension_type.id() as i32,
                dimension_name: new_world.key.clone(),
                hashed_seed: new_world.obfuscated_seed(),
                gamemode: player.game_mode() as u8,
                previous_gamemode: player.previous_game_mode().map_or(-1, |g| g as i8),
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
        let world = self.entity.lock().get_world();

        // Body: single lock, sending position/abilities/time/weather/context to
        // the player. None of these re-lock this entity (`resend_player_context`
        // takes `&Player`), so holding the guard is safe.
        let (entity_id, player_name) = {
            let mut player = self.entity.lock();

            // Set position and rotation
            player.base.set_position_local(position);
            player.set_rotation(rotation);
            player.set_old_position_to_current();
            player.movement.reset_for_position_sync(position);

            // Teleport sync (sends CPlayerPosition, sets awaiting_teleport for ack)
            if let Err(error) =
                player.teleport(position.x, position.y, position.z, rotation.0, rotation.1)
            {
                panic!(
                    "failed to synchronize player {} spawn position: {error}",
                    player.id()
                );
            }
            player.reset_flying_ticks();

            // Abilities and held slot
            player.send_abilities();
            player.send_packet(CSetHeldSlot {
                slot: i32::from(player.inventory.lock().get_selected_slot()),
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
                player.send_packet(CSetTime::new(game_time, day_time, 0.0, rate));
            }

            player.send_packet(world.initialize_border_packet());

            // Weather sync
            if world.can_have_weather() && world.is_raining() {
                let (rain_level, thunder_level) = {
                    let weather = world.weather.lock();
                    (weather.rain_level, weather.thunder_level)
                };

                player.send_packet(CGameEvent {
                    event: GameEventType::StartRaining,
                    data: 0.0,
                });
                player.send_packet(CGameEvent {
                    event: GameEventType::RainLevelChange,
                    data: rain_level,
                });
                player.send_packet(CGameEvent {
                    event: GameEventType::ThunderLevelChange,
                    data: thunder_level,
                });
            }

            // Force health/xp resync on next tick
            player.reset_sent_info();

            // Resend client context that is not fully covered by CLogin/CRespawn.
            player.server().resend_player_context(&player);

            (player.id(), player.gameprofile.name.clone())
        };

        // Add to world / re-enter chunk tracking. These re-lock this entity, so
        // run them with the guard dropped.
        match reason {
            ResetReason::InitialJoin | ResetReason::WorldChange => {
                if reason == ResetReason::WorldChange {
                    log::info!("Player {} changed world to {}", player_name, world.key);
                }
                world.add_player(self.clone(), reason)
            }
            ResetReason::Respawn => {
                // Same world — re-enter chunk tracking
                world.player_area_map.remove_by_entity_id(entity_id);
                world.chunk_map.remove_player(self);
                world.entity_tracker().on_player_leave(entity_id);

                self.entity.lock().send_packet(CGameEvent {
                    event: GameEventType::LevelChunksLoadStart,
                    data: 0.0,
                });
                world.register_respawned_player_entity(self);
                true
            }
        }
    }

    /// Runs the deferred respawn tail (reset + spawn) for [`Player::respawn`].
    ///
    /// Called from a tick safe point with the entity unlocked, since `reset`/`spawn`
    /// re-lock it. The `&mut self` prep (state reset, unregister) already ran inline.
    pub(crate) fn finish_respawn(self: &Arc<Self>) {
        let world = self.entity.lock().get_world();

        // Shared reset (clears transient state, sends CRespawn)
        self.reset(world.clone(), ResetReason::Respawn);

        // Compute spawn position
        let spawn_pos = world.level_data.read().data().spawn_pos();
        let spawn = DVec3::new(
            f64::from(spawn_pos.x()) + 0.5,
            f64::from(spawn_pos.y()),
            f64::from(spawn_pos.z()) + 0.5,
        );

        {
            let mut player = self.entity.lock();
            player.send_difficulty();

            // Handle XP loss on death
            if world.get_game_rule(&KEEP_INVENTORY) != GameRuleValue::Bool(true)
                && player.game_mode() != GameType::Spectator
            {
                // TODO: drop XP orbs (min(level * 7, 100))
                player.experience.set_total_points(0);
            }
            // Re-send XP to client after respawn regardless of keepInventory
            player.experience.dirty = true;
        }

        // Shared spawn (teleport, abilities, weather, time, chunk tracking reset)
        let _ = self.spawn(spawn, (0.0, 0.0), ResetReason::Respawn);
    }

    /// Runs the deferred cross-world change tail (reset + spawn) for
    /// [`Player::change_world`], at a tick safe point with the entity unlocked.
    pub(crate) fn finish_world_change(self: &Arc<Self>, transition: &TeleportTransition) {
        let new_world = transition.target_world.clone();
        self.reset(new_world, ResetReason::WorldChange);
        // TODO: set portal cooldown from transition.portal_cooldown
        if !self.spawn(
            transition.position,
            transition.rotation,
            ResetReason::WorldChange,
        ) {
            return;
        }
        // Vanilla: PlayerList.sendAllPlayerInfo -> inventoryMenu.sendAllDataToRemote
        self.entity.lock().send_inventory_to_remote();
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
    fn base_weak(&self) -> &Weak<EntityBase> {
        &self.base_weak
    }

    fn base(&self) -> Arc<EntityBase> {
        self.base.clone()
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::PLAYER
    }

    fn stop_riding(&self) {
        let old_vehicle = self.vehicle();
        self.base().stop_riding();
        let Some(old_vehicle) = old_vehicle else {
            return;
        };

        let passenger_ids = old_vehicle.with_entity(|vehicle| {
            self.remove_active_effects_for_vehicle(vehicle);
            Self::passenger_ids_for_packet(vehicle)
        });
        self.send_packet(CSetPassengers::new(old_vehicle.id(), passenger_ids));
    }

    fn start_riding(&mut self, entity_to_ride: &mut dyn Entity) -> bool {
        if !start_riding_entities(self, entity_to_ride) {
            return false;
        }

        let vehicle_id = entity_to_ride.id();
        entity_to_ride.position_rider(self.as_entity_event_source_mut());
        let position = self.position();
        let (yaw, pitch) = self.rotation();
        if let Err(error) = self.teleport(position.x, position.y, position.z, yaw, pitch) {
            panic!(
                "failed to synchronize player {} mounted position: {error}",
                self.id()
            );
        }
        self.send_active_effects_for_vehicle(&*entity_to_ride);
        let passenger_ids = Self::passenger_ids_for_packet(&*entity_to_ride);
        self.send_packet(CSetPassengers::new(vehicle_id, passenger_ids));
        true
    }

    fn broadcast_to_player(&self, player: &Player) -> bool {
        if player.is_spectator() {
            true
        } else {
            !self.is_spectator()
        }
    }

    fn tick(&mut self) {
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

    fn as_living_entity(&self) -> Option<&dyn LivingEntity> {
        Some(self)
    }

    fn as_player(&mut self) -> Option<&mut Player> {
        Some(self)
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

    fn is_pushable(&mut self) -> bool {
        self.get_health() > 0.0 && !self.is_spectator() && !self.on_climbable()
    }

    fn on_climbable(&mut self) -> bool {
        Player::on_climbable(self)
    }

    fn is_spectator(&self) -> bool {
        self.game_mode() == GameType::Spectator
    }

    fn fire_immune_ticks(&self) -> i32 {
        20
    }

    fn remaining_fire_ticks_cap(&self) -> Option<i32> {
        self.abilities.invulnerable.then_some(1)
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

    fn is_flying_player(&self) -> bool {
        self.is_flying()
    }

    fn is_effective_ai(&self) -> bool {
        true
    }

    fn known_movement(&self) -> DVec3 {
        if let Some(vehicle) = self.vehicle()
            && let Some(movement) = vehicle.with_entity(|v| {
                v.controlling_passenger_for_rider(self)
                    .is_none_or(|controller| controller.id() != self.id())
                    .then(|| v.known_movement())
            })
        {
            return movement;
        }

        self.movement.last_known_client_movement()
    }

    fn known_speed(&self) -> DVec3 {
        if let Some(vehicle) = self.vehicle()
            && vehicle
                .controlling_passenger_for_rider(self)
                .is_none_or(|controller| controller.id() != self.id())
        {
            return vehicle.known_speed();
        }

        self.movement.last_known_client_movement()
    }

    fn is_suppressing_bounce(&self) -> bool {
        self.is_crouching()
    }

    fn cause_fall_damage(
        &mut self,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
        rider: Option<&mut dyn Entity>,
    ) -> bool {
        if self.abilities.may_fly {
            return false;
        }

        // TODO: Award `Stats.FALL_ONE_CM` once player statistics are implemented.
        LivingEntity::cause_living_fall_damage(self, fall_distance, damage_modifier, source, rider)
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn update_data_before_sync(&mut self) {
        self.update_dirty_mob_effect_entity_data();
    }

    fn pack_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
        self.attributes().syncable_snapshots()
    }

    fn drain_dirty_syncable_attributes(&mut self) -> Vec<AttributeSnapshot> {
        self.attributes_mut().drain_dirty_sync()
    }

    fn drain_dirty_mob_effects(&mut self) -> Vec<MobEffectSyncChange> {
        self.living_base.drain_dirty_mob_effects().collect()
    }

    fn pack_all_equipment(&self) -> Vec<EquipmentSlotItem> {
        equipment_items_to_packet_items(self.inventory.lock().non_empty_equipment_items())
    }

    fn drain_dirty_equipment(&mut self) -> Vec<EquipmentSlotItem> {
        equipment_items_to_packet_items(self.inventory.lock().drain_dirty_equipment_items())
    }

    fn max_up_step(&self) -> f32 {
        self.attributes()
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

    fn on_below_world(&mut self) {
        self.hurt(
            &DamageSource::environment(&vanilla_damage_types::OUT_OF_WORLD),
            4.0,
        );
    }

    fn dimensions_for_pose(&self, pose: EntityPose) -> EntityDimensions {
        let dimensions = Player::dimensions_for_pose(pose);
        if pose == EntityPose::Sleeping || self.entity_type().fixed {
            dimensions
        } else {
            dimensions.scale(LivingEntity::get_scale(self))
        }
    }

    fn hurt(&mut self, source: &DamageSource, amount: f32) -> bool {
        // Delegates to Player's inherent hurt method which handles
        // player-specific prechecks before the shared living hurt path.
        Player::hurt(self, source, amount)
    }

    fn change_world(&mut self, teleport_transition: &TeleportTransition) {
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
            // The reset+spawn tail re-locks this entity (currently locked by the
            // world-change processor), so defer it to a safe point.
            // See `ServerPlayer::finish_world_change`.
            self.server()
                .queue_player_reset(PlayerResetRequest::WorldChange(
                    self.server_player(),
                    teleport_transition.clone(),
                ));
        }
    }
}

impl LivingEntity for Player {
    fn living_base(&mut self) -> &mut LivingEntityBase {
        &mut self.living_base
    }

    fn living_base_ref(&self) -> &LivingEntityBase {
        &self.living_base
    }

    fn get_health(&self) -> f32 {
        *self.entity_data.living_entity().health.get()
    }

    fn set_health(&mut self, health: f32) {
        let max_health = self.get_max_health();
        let clamped = health.clamp(0.0, max_health);
        self.entity_data.living_entity_mut().health.set(clamped);
    }

    fn can_be_seen_as_enemy(&self) -> bool {
        !self.abilities.invulnerable && !self.is_invulnerable() && self.can_be_seen_by_anyone()
    }

    fn is_invulnerable_to(&self, source: &DamageSource) -> bool {
        if self.default_is_invulnerable_to(source)
            || enchantment_helper::is_immune_to_damage(self, source)
        {
            return true;
        }

        // TODO: apply drowningDamage, fallDamage, fireDamage, and freezeDamage gamerules.
        !self.has_client_loaded()
    }

    fn actually_hurt(&mut self, source: &DamageSource, amount: f32) {
        Player::actually_hurt(self, source, amount);
    }

    fn hurt_broadcast_chunk(&self) -> ChunkPos {
        self.view().last_chunk_pos()
    }

    fn die(&mut self, source: &DamageSource) {
        Player::die(self, source);
    }

    fn with_equipment_slot(&self, slot: EquipmentSlot, visitor: &mut dyn FnMut(&ItemStack)) {
        let inventory = self.inventory.lock();
        if slot == EquipmentSlot::MainHand {
            visitor(inventory.get_selected_item());
        } else {
            visitor(inventory.equipment().get_ref(slot));
        }
    }

    fn with_equipment_slot_mut(
        &mut self,
        slot: EquipmentSlot,
        visitor: &mut dyn FnMut(&mut ItemStack),
    ) {
        let mut inventory = self.inventory.lock();
        if slot == EquipmentSlot::MainHand {
            visitor(inventory.get_selected_item_mut());
        } else {
            visitor(inventory.equipment_mut().get_mut(slot));
        }
    }

    fn has_infinite_materials(&self) -> bool {
        Player::has_infinite_materials(self)
    }

    fn get_absorption_amount(&self) -> f32 {
        *self.entity_data.player_absorption.get()
    }

    fn set_absorption_amount(&mut self, amount: f32) {
        self.entity_data.player_absorption.set(amount.max(0.0));
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

    fn jump_from_ground(&mut self) {
        self.default_jump_from_ground();
        // TODO: Award Stats.JUMP once player statistics exist.
        if self.is_sprinting() {
            self.cause_food_exhaustion(0.2);
        } else {
            self.cause_food_exhaustion(0.05);
        }
    }

    fn ai_step(&mut self) -> Option<MoveResult> {
        if self.is_flying() && !self.is_passenger() {
            self.reset_fall_distance();
        }

        let result = self.default_ai_step();
        self.set_y_head_rot(self.rotation().0);
        result
    }

    fn travel(&mut self, input: DVec3) -> Option<MoveResult> {
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
            let flying_speed = self.abilities.flying_speed;
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
