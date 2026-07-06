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
mod spam_throttler;
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
    packets::game::{CLevelEvent, CSetEntityData, CSetExperience},
};
use teleport_state::TeleportState;
use tick_state::PlayerTickState;

use block_breaking::BlockBreakingManager;
use enum_dispatch::enum_dispatch;
use game_mode_state::PlayerGameModeState;
pub use game_profile::{GameProfile, GameProfileAction};
use std::sync::{Arc, Weak};
use steel_macros::entity_impl;
use steel_protocol::packets::game::{
    AttributeSnapshot, CEntityEvent, CPlayerCombatKill, CRespawn, CSetDefaultSpawnPosition,
    CSetHealth, CSetHeldSlot, CSetPassengers, CSetTime, ClientCommandAction, EquipmentSlotItem,
    RelativeMovement, SoundSource,
};
use steel_registry::RegistryEntry;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_data::EntityPose;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::game_rules::{GameRuleRef, GameRuleValue};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entity_data::PlayerEntityData;
use steel_registry::vanilla_game_rules::{
    ADVANCE_TIME, DROWNING_DAMAGE, FALL_DAMAGE, FIRE_DAMAGE, FREEZE_DAMAGE, IMMEDIATE_RESPAWN,
    KEEP_INVENTORY, SHOW_DEATH_MESSAGES,
};
use steel_registry::{
    level_events, sound_events, vanilla_attributes, vanilla_damage_type_tags, vanilla_entities,
    vanilla_particle_types,
};
use steel_utils::entity_events::EntityStatus;

use arc_swap::ArcSwap;
use steel_utils::locks::SyncMutex;
use steel_utils::types::{Difficulty, GameType};
use text_components::TextComponent;
use text_components::resolving::TextResolutor;
use text_components::translation::TranslatedMessage;
use text_components::{content::Resolvable, custom::CustomData};

use crate::chunk::chunk_request::{ChunkRequestHandle, ChunkRequestState};
use crate::config::RuntimeConfig;
use crate::enchantment_helper;
use crate::entity::damage::DamageSource;
use crate::entity::{
    DEATH_DURATION, Entity, EntityBase, EntityEventSource, EntityMovementEmission,
    EntitySyncedData, LivingEntity, LivingEntityBase, MobEffectSyncChange, MobEffectSyncPacket,
    RemovalReason, SharedEntity, equipment_items_to_packet_items, start_riding_entities,
};
use crate::fluid::get_fluid_state;
use crate::inventory::{SyncPlayerInv, equipment::EquipmentSlot};
use crate::level_data::RespawnData;
use crate::physics::MoveResult;
use crate::player::experience::Experience;
use crate::player::player_data::PersistentRootVehicle;
use crate::player::player_inventory::PlayerInventory;
use crate::server::{
    Server,
    jobs::{JobPoll, ServerJob, ServerJobContext},
};
use crate::world::player_spawn_finder::{PlayerSpawnSearch, PlayerSpawnSearchPoll};
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

const RESPAWN_SEARCH_READY_CANDIDATE_BUDGET: usize = 8;

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
use crate::portal::{
    PortalTicketTarget, TeleportPostAction, TeleportPostTransition, TeleportTransition,
};
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

    /// Last main-hand stack used for vanilla attack-strength reset checks.
    last_item_in_main_hand: SyncMutex<ItemStack>,

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

    /// Whether the player has completed the vanilla End credits flow.
    seen_credits: SyncMutex<bool>,

    /// Vanilla `ServerPlayer.wonGame`; transient while the End credits screen is open.
    won_game: SyncMutex<bool>,

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

#[derive(Clone, Copy)]
struct DeathRespawnSpawn {
    position: DVec3,
    rotation: (f32, f32),
}

struct PlayerRespawnJob {
    player: Arc<Player>,
    source_world: Arc<World>,
    target_world: Arc<World>,
    rotation: (f32, f32),
    kind: RespawnRequestKind,
    phase: PlayerRespawnJobPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RespawnRequestKind {
    Death,
    EndCredits,
}

enum PlayerRespawnJobPhase {
    Searching(PlayerSpawnSearch),
    LoadingSpawnChunks {
        spawn: DeathRespawnSpawn,
        request: ChunkRequestHandle,
    },
}

impl PlayerRespawnJob {
    fn new(
        player: Arc<Player>,
        source_world: Arc<World>,
        target_world: Arc<World>,
        respawn_data: RespawnData,
        kind: RespawnRequestKind,
    ) -> Result<Self, String> {
        let search = PlayerSpawnSearch::new(
            &target_world,
            respawn_data.pos(),
            target_world.default_gamemode,
        )?;
        Ok(Self {
            player,
            source_world,
            target_world,
            rotation: (respawn_data.yaw, respawn_data.pitch),
            kind,
            phase: PlayerRespawnJobPhase::Searching(search),
        })
    }

    fn still_valid(&self) -> bool {
        !self.player.connection.closed()
            && Arc::ptr_eq(&self.player.get_world(), &self.source_world)
            && match self.kind {
                RespawnRequestKind::Death => {
                    Player::should_process_respawn(self.player.get_health())
                }
                RespawnRequestKind::EndCredits => self.player.has_won_game(),
            }
    }
}

impl ServerJob for PlayerRespawnJob {
    fn poll(&mut self, _context: &mut ServerJobContext) -> JobPoll {
        if !self.still_valid() {
            self.player.finish_respawn_request();
            return JobPoll::Finished;
        }

        loop {
            match &mut self.phase {
                PlayerRespawnJobPhase::Searching(search) => {
                    match search.poll_with_ready_candidate_budget(
                        &self.target_world,
                        RESPAWN_SEARCH_READY_CANDIDATE_BUDGET,
                    ) {
                        PlayerSpawnSearchPoll::Pending => return JobPoll::Pending,
                        PlayerSpawnSearchPoll::Cancelled => {
                            self.player.finish_respawn_request();
                            return JobPoll::Finished;
                        }
                        PlayerSpawnSearchPoll::Ready(position) => {
                            let spawn = DeathRespawnSpawn {
                                position,
                                rotation: self.rotation,
                            };
                            let request = self.target_world.request_player_spawn_chunks(position);
                            self.phase =
                                PlayerRespawnJobPhase::LoadingSpawnChunks { spawn, request };
                        }
                    }
                }
                PlayerRespawnJobPhase::LoadingSpawnChunks { spawn, request } => {
                    match request.poll() {
                        ChunkRequestState::Pending { .. } => return JobPoll::Pending,
                        ChunkRequestState::Cancelled => {
                            self.player.finish_respawn_request();
                            return JobPoll::Finished;
                        }
                        ChunkRequestState::Ready => {
                            if request.ready_chunks().is_none() {
                                return JobPoll::Pending;
                            }

                            match self.kind {
                                RespawnRequestKind::Death => self.player.finish_death_respawn(
                                    &self.source_world,
                                    &self.target_world,
                                    *spawn,
                                ),
                                RespawnRequestKind::EndCredits => {
                                    self.player.finish_end_credits_respawn(
                                        &self.source_world,
                                        &self.target_world,
                                        *spawn,
                                    );
                                }
                            }
                            return JobPoll::Finished;
                        }
                    }
                }
            }
        }
    }

    fn cancel(&mut self) {
        self.player.finish_respawn_request();
    }
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
    pub fn previous_game_mode(&self) -> Option<GameType> {
        self.game_modes.lock().previous()
    }

    /// Restores current and previous game mode from persistent player data.
    pub(crate) fn restore_game_modes(&self, current: GameType, previous: Option<GameType>) {
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
        let chat_spam_threshold_seconds = config.chat_spam_threshold_seconds;
        let command_spam_threshold_seconds = config.command_spam_threshold_seconds;

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
            chat: SyncMutex::new(ChatState::new(
                chat_spam_threshold_seconds,
                command_spam_threshold_seconds,
            )),
            game_modes: SyncMutex::new(PlayerGameModeState::new(GameType::Survival)),
            inventory: inventory.clone(),
            last_item_in_main_hand: SyncMutex::new(ItemStack::empty()),
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
            seen_credits: SyncMutex::new(false),
            won_game: SyncMutex::new(false),
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
        self.tick_attack_strength();
        self.tick_spam_throttlers();
        self.tick_client_load_timeout();

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

        self.living_base.decrement_invulnerable_time();
        self.tick_mob_effects();

        if self.get_health() <= 0.0 {
            self.tick_death();
        } else {
            let world = self.get_world();
            self.touch_nearby_items();
            self.block_breaking.lock().tick(self, &world);

            // TODO: Implement remaining player ticking logic here
            // - Managing game mode specific logic
            // - Updating advancements
            // - Handling falling

            self.update_player_attributes();
            self.living_base.refresh_speed_from_attributes();
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

    fn refresh_equipment_attribute_modifiers_from_stack(
        &self,
        slot: EquipmentSlot,
        item_stack: &ItemStack,
    ) {
        self.living_base
            .refresh_equipment_attribute_modifiers(slot, item_stack);
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

    fn update_dirty_mob_effect_entity_data(&self) {
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
            let mut entity_data = self.entity_data.lock();
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
    pub fn handle_client_tick_end(&self) {
        self.movement.lock().finish_client_tick();
    }

    /// Main entry point for dealing damage. Returns `true` if damage was applied.
    pub fn hurt(&self, source: &DamageSource, amount: f32) -> bool {
        if LivingEntity::is_invulnerable_to(self, source) {
            return false;
        }

        {
            let abilities = self.abilities.lock();
            if abilities.invulnerable && !source.bypasses_invulnerability() {
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

    fn disabled_damage_game_rule(source: &DamageSource) -> Option<GameRuleRef> {
        if source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_DROWNING) {
            Some(&DROWNING_DAMAGE)
        } else if source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FALL) {
            Some(&FALL_DAMAGE)
        } else if source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FIRE) {
            Some(&FIRE_DAMAGE)
        } else if source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FREEZING) {
            Some(&FREEZE_DAMAGE)
        } else {
            None
        }
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

        self.clear_fire();
        self.set_ticks_frozen(0);

        if world.get_game_rule(&IMMEDIATE_RESPAWN) == GameRuleValue::Bool(true) {
            self.respawn();
        }
    }

    /// TODO: personal respawn blocks/anchors and noRespawnBlockAvailable.
    pub fn respawn(&self) {
        let health = self.get_health();
        if !Self::should_process_respawn(health) {
            return;
        }

        let source_world = self.get_world();
        let Some(player_arc) = source_world.players.get_by_entity_id(self.id()) else {
            return;
        };
        if !self.begin_respawn_request() {
            return;
        }

        let Some(server) = self.server.upgrade() else {
            self.finish_respawn_request();
            log::error!(
                "Failed to schedule respawn for player {}: server is gone",
                self.gameprofile.name
            );
            return;
        };
        let (target_world, respawn_data) =
            match server.respawn_world_and_data_for_domain(source_world.domain()) {
                Ok(resolved) => resolved,
                Err(error) => {
                    self.finish_respawn_request();
                    log::error!(
                        "Failed to schedule respawn for player {}: {error}",
                        self.gameprofile.name
                    );
                    return;
                }
            };

        match PlayerRespawnJob::new(
            player_arc,
            source_world,
            target_world,
            respawn_data,
            RespawnRequestKind::Death,
        ) {
            Ok(job) => server.jobs.spawn(job),
            Err(error) => {
                self.finish_respawn_request();
                log::error!(
                    "Failed to schedule respawn for player {}: {error}",
                    self.gameprofile.name
                );
            }
        }
    }

    fn finish_death_respawn(
        self: &Arc<Self>,
        source_world: &Arc<World>,
        target_world: &Arc<World>,
        spawn: DeathRespawnSpawn,
    ) {
        self.finish_respawn_request();

        if self.connection.closed()
            || !Arc::ptr_eq(&self.get_world(), source_world)
            || !Self::should_process_respawn(self.get_health())
        {
            return;
        }

        self.reset_state_for_death_respawn();
        let was_removed = self.base.clear_removed();

        // TODO: personal respawn blocks/anchors and NO_RESPAWN_BLOCK_AVAILABLE.

        if !was_removed && Arc::ptr_eq(source_world, target_world) {
            source_world.unregister_player_entity(self);
        }

        // Shared reset (clears transient state, sends CRespawn)
        self.reset(target_world.clone(), ResetReason::Respawn);

        self.send_difficulty();

        // Handle XP loss on death
        {
            let mut experience = self.experience.lock();
            if target_world.get_game_rule(&KEEP_INVENTORY) != GameRuleValue::Bool(true)
                && self.game_mode() != GameType::Spectator
            {
                // TODO: drop XP orbs (min(level * 7, 100))
                experience.set_total_points(0);
            }
            // Re-send XP to client after respawn regardless of keepInventory
            experience.dirty = true;
        }

        // TODO: send mob effect packets once effects are implemented

        // Shared spawn (teleport, abilities, weather, time, chunk tracking reset)
        let _ = self.spawn(spawn.position, spawn.rotation, ResetReason::Respawn);
    }

    fn finish_end_credits_respawn(
        self: &Arc<Self>,
        source_world: &Arc<World>,
        target_world: &Arc<World>,
        spawn: DeathRespawnSpawn,
    ) {
        self.finish_respawn_request();

        if self.connection.closed()
            || !Arc::ptr_eq(&self.get_world(), source_world)
            || !self.has_won_game()
        {
            return;
        }

        self.set_won_game(false);
        self.reset(target_world.clone(), ResetReason::EndCredits);
        self.send_difficulty();
        self.experience.lock().dirty = true;
        let _ = self.spawn(spawn.position, spawn.rotation, ResetReason::EndCredits);
    }

    fn reset_state_for_death_respawn(&self) {
        self.close_container();
        self.detach_relationships_for_respawn();

        self.attributes().lock().remove_all_transient();
        self.living_base.reset_for_player_respawn();
        self.base
            .reset_for_player_respawn(Self::dimensions_for_pose(EntityPose::Standing));

        self.set_health(self.get_max_health());
        self.set_pose(EntityPose::Standing);
        self.reset_entity_state();
        self.sync_base_entity_data();
        self.update_dirty_mob_effect_entity_data();

        *self.food_data.lock() = FoodData::new();
        *self.block_breaking.lock() = BlockBreakingManager::new();
        *self.teleport_state.lock() = TeleportState::new();
        *self.tick_state.lock() = PlayerTickState::new();
        *self.last_item_in_main_hand.lock() = ItemStack::empty();
        self.health_sync.lock().reset_for_respawn();
        self.clear_pending_root_vehicle();
        self.movement.lock().reset_last_known_client_movement();
    }

    fn begin_respawn_request(&self) -> bool {
        self.lifecycle.lock().begin_respawn()
    }

    fn finish_respawn_request(&self) {
        self.lifecycle.lock().finish_respawn();
    }

    fn detach_relationships_for_respawn(&self) {
        for passenger in self.passengers() {
            passenger.stop_riding();
        }
        self.stop_riding();
        self.base.set_boarding_cooldown(0);
    }

    /// Handles client commands, requestStats and `RequestGameRuleValues` are still todo
    pub fn handle_client_command(self: &Arc<Self>, action: ClientCommandAction) {
        match action {
            ClientCommandAction::PerformRespawn => {
                if self.has_won_game() {
                    self.respawn_after_end_credits();
                } else {
                    self.respawn();
                }
            }
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

    /// Returns vanilla `ServerPlayer.seenCredits`.
    #[must_use]
    pub fn has_seen_credits(&self) -> bool {
        *self.seen_credits.lock()
    }

    /// Sets vanilla `ServerPlayer.seenCredits`.
    pub fn set_seen_credits(&self, seen_credits: bool) {
        *self.seen_credits.lock() = seen_credits;
    }

    /// Returns vanilla `ServerPlayer.wonGame`.
    #[must_use]
    pub(crate) fn has_won_game(&self) -> bool {
        *self.won_game.lock()
    }

    fn set_won_game(&self, won_game: bool) {
        *self.won_game.lock() = won_game;
    }

    /// Starts the vanilla End credits flow.
    pub(crate) fn show_end_credits(&self) {
        let world = self.get_world();
        let Some(player) = world.players.get_by_entity_id(self.id()) else {
            return;
        };

        world.remove_player_for_world_change(&player);
        if player.has_won_game() {
            return;
        }

        player.set_won_game(true);
        player.send_packet(CGameEvent {
            event: GameEventType::WinGame,
            data: 0.0,
        });
        player.set_seen_credits(true);
    }

    fn respawn_after_end_credits(self: &Arc<Self>) {
        if !self.has_won_game() {
            return;
        }

        let source_world = self.get_world();
        if !self.begin_respawn_request() {
            return;
        }

        let Some(server) = self.server.upgrade() else {
            self.finish_respawn_request();
            log::error!(
                "Failed to schedule End credits respawn for player {}: server is gone",
                self.gameprofile.name
            );
            return;
        };
        let (target_world, respawn_data) =
            match server.respawn_world_and_data_for_domain(source_world.domain()) {
                Ok(resolved) => resolved,
                Err(error) => {
                    self.finish_respawn_request();
                    log::error!(
                        "Failed to schedule End credits respawn for player {}: {error}",
                        self.gameprofile.name
                    );
                    return;
                }
            };

        match PlayerRespawnJob::new(
            Arc::clone(self),
            source_world,
            target_world,
            respawn_data,
            RespawnRequestKind::EndCredits,
        ) {
            Ok(job) => server.jobs.spawn(job),
            Err(error) => {
                self.finish_respawn_request();
                log::error!(
                    "Failed to schedule End credits respawn for player {}: {error}",
                    self.gameprofile.name
                );
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

    /// Returns vanilla `Player.takeXpDelay`.
    #[must_use]
    pub(crate) fn take_xp_delay(&self) -> i32 {
        self.tick_state.lock().take_xp_delay()
    }

    /// Sets vanilla `Player.takeXpDelay`.
    pub(crate) fn set_take_xp_delay(&self, delay: i32) {
        self.tick_state.lock().set_take_xp_delay(delay);
    }

    /// Gives raw experience points to this player.
    pub(crate) fn give_experience_points(&self, points: i32) {
        self.experience.lock().add_points(points);
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

        if reason != ResetReason::InitialJoin {
            // 0x01 = keep attributes, 0x02 = keep entity data
            let data_kept = reason.respawn_data_kept();

            self.send_packet(CRespawn {
                dimension_type: new_world.dimension_type.id() as i32,
                dimension_name: new_world.key.clone(),
                hashed_seed: new_world.obfuscated_seed(),
                gamemode: self.game_mode() as u8,
                previous_gamemode: nullable_game_mode_id(self.previous_game_mode()),
                is_debug: false,
                is_flat: new_world.is_flat,
                has_death_location: false,
                death_dimension_name: None,
                death_location: None,
                portal_cooldown_ticks: self.portal_cooldown(),
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
        self.spawn_with_velocity(position, rotation, DVec3::ZERO, reason)
    }

    #[must_use]
    pub(crate) fn spawn_with_velocity(
        self: &Arc<Self>,
        position: DVec3,
        rotation: (f32, f32),
        velocity: DVec3,
        reason: ResetReason,
    ) -> bool {
        self.spawn_with_velocity_packet(
            position,
            rotation,
            velocity,
            reason,
            position,
            rotation,
            velocity,
            RelativeMovement::NONE,
        )
    }

    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "packet-relative teleports must keep resolved and protocol values separate"
    )]
    pub(crate) fn spawn_with_velocity_packet(
        self: &Arc<Self>,
        position: DVec3,
        rotation: (f32, f32),
        velocity: DVec3,
        reason: ResetReason,
        packet_position: DVec3,
        packet_rotation: (f32, f32),
        packet_velocity: DVec3,
        relatives: RelativeMovement,
    ) -> bool {
        let world = self.get_world();

        // Set position and rotation
        self.base.set_position_local(position);
        self.set_rotation(rotation);
        self.set_old_position_to_current();
        self.movement.lock().reset_for_position_sync(position);

        // Teleport sync (sends CPlayerPosition, sets awaiting_teleport for ack)
        if let Err(error) = self.teleport_with_velocity_packet(
            position,
            velocity,
            rotation,
            packet_position,
            packet_velocity,
            packet_rotation,
            relatives,
        ) {
            panic!(
                "failed to synchronize player {} spawn position: {error}",
                self.id()
            );
        }
        self.reset_flying_ticks();

        self.send_spawn_state_packets(&world);

        // Force health/xp resync on next tick
        self.reset_sent_info();

        // Resend client context that is not fully covered by CLogin/CRespawn.
        self.server().resend_player_context(self);
        self.send_active_effects_for_self();

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
            ResetReason::Respawn | ResetReason::EndCredits => {
                if world.players.get_by_entity_id(self.id()).is_none() {
                    return world.add_respawned_player(self.clone());
                }

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

    fn send_spawn_state_packets(&self, world: &World) {
        self.send_abilities();
        self.send_packet(CSetHeldSlot {
            slot: i32::from(self.inventory.lock().get_selected_slot()),
        });
        self.send_time_sync(world);
        self.send_packet(world.initialize_border_packet());
        self.send_default_spawn_position(world);
        self.send_weather_sync(world);
    }

    fn send_time_sync(&self, world: &World) {
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

    fn send_default_spawn_position(&self, world: &World) {
        if let Some(server) = self.server.upgrade() {
            match server.respawn_data_for_domain(world.domain()) {
                Ok(respawn_data) => {
                    self.send_packet(CSetDefaultSpawnPosition {
                        global_pos: respawn_data.global_pos,
                        yaw: respawn_data.yaw,
                        pitch: respawn_data.pitch,
                    });
                }
                Err(error) => {
                    log::error!(
                        "Failed to send default spawn position to player {}: {error}",
                        self.gameprofile.name
                    );
                }
            }
        }
    }

    fn send_weather_sync(&self, world: &World) {
        if !world.can_have_weather() || !world.is_raining() {
            return;
        }

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

    fn send_active_effects_for_self(&self) {
        for effect in self.living_base.active_mob_effects() {
            self.send_mob_effect_sync_packet(
                MobEffectSyncChange::Update {
                    effect,
                    blend_for_self: false,
                }
                .packet(self.id(), true),
            );
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

    pub(crate) fn send_restored_vehicle_mount_sync(&self, vehicle: &dyn Entity) {
        self.send_active_effects_for_vehicle(vehicle);
        self.send_packet(CSetPassengers::new(
            vehicle.id(),
            Self::passenger_ids_for_packet(vehicle),
        ));
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

    fn apply_post_teleport_transition(&self, post_transition: &TeleportPostTransition) {
        for action in post_transition.actions() {
            match *action {
                TeleportPostAction::PlayPortalSound => {
                    self.send_packet(CLevelEvent::new(
                        level_events::SOUND_PORTAL_TRAVEL,
                        BlockPos::ZERO,
                        0,
                        false,
                    ));
                }
                TeleportPostAction::PlacePortalTicket(target) => {
                    let ticket_position = match target {
                        PortalTicketTarget::Destination => BlockPos::from(self.position()),
                        PortalTicketTarget::Block(pos) => pos,
                    };
                    self.get_world().place_portal_ticket(ticket_position);
                }
            }
        }
    }
}

fn nullable_game_mode_id(game_mode: Option<GameType>) -> i8 {
    game_mode.map_or(-1, |game_mode| game_mode as i8)
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
    /// Respawning after the End credits screen with vanilla packet flags.
    EndCredits,
    /// Teleporting to a different loaded world.
    WorldChange,
}

impl ResetReason {
    const fn respawn_data_kept(self) -> i8 {
        match self {
            Self::InitialJoin | Self::Respawn => 0x00,
            Self::EndCredits => 0x01,
            Self::WorldChange => 0x03,
        }
    }
}

#[entity_impl(class(player))]
impl Entity for Player {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::PLAYER
    }

    fn is_always_ticking(&self) -> bool {
        true
    }

    fn update_swimming(&self) {
        if self.is_flying() {
            self.set_shared_swimming(false);
        } else {
            self.default_update_swimming();
        }
    }

    fn stop_riding(&self) {
        let old_vehicle = self.vehicle();
        self.base().stop_riding();
        let Some(old_vehicle) = old_vehicle else {
            return;
        };

        self.remove_active_effects_for_vehicle(old_vehicle.as_ref());
        self.send_packet(CSetPassengers::new(
            old_vehicle.id(),
            Self::passenger_ids_for_packet(old_vehicle.as_ref()),
        ));
    }

    fn start_riding(&self, entity_to_ride: &SharedEntity) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        let Some(passenger) = world.get_entity_by_id(self.id()) else {
            return false;
        };
        if !start_riding_entities(&passenger, entity_to_ride) {
            return false;
        }

        entity_to_ride.position_rider(self.as_entity_event_source());
        let position = self.position();
        let (yaw, pitch) = self.rotation();
        if let Err(error) = self.teleport(position, yaw, pitch) {
            panic!(
                "failed to synchronize player {} mounted position: {error}",
                self.id()
            );
        }
        self.send_active_effects_for_vehicle(entity_to_ride.as_ref());
        self.send_packet(CSetPassengers::new(
            entity_to_ride.id(),
            Self::passenger_ids_for_packet(entity_to_ride.as_ref()),
        ));
        true
    }

    fn broadcast_to_player(&self, player: &Player) -> bool {
        if player.is_spectator() {
            true
        } else {
            !self.is_spectator()
        }
    }

    fn tick(&self) {
        Player::tick(self);
    }

    fn fall_sounds(&self) -> (SoundEventRef, SoundEventRef) {
        (
            &sound_events::ENTITY_PLAYER_SMALL_FALL,
            &sound_events::ENTITY_PLAYER_BIG_FALL,
        )
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

    fn is_flying_player(&self) -> bool {
        self.is_flying()
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
        rand::random_range(1..=2)
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
        LivingEntity::cause_living_fall_damage(self, fall_distance, damage_modifier, source)
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn update_data_before_sync(&self) {
        self.update_dirty_mob_effect_entity_data();
    }

    fn pack_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
        self.attributes().lock().syncable_snapshots()
    }

    fn drain_dirty_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
        self.attributes().lock().drain_dirty_sync()
    }

    fn drain_dirty_mob_effects(&self) -> Vec<MobEffectSyncChange> {
        self.living_base.drain_dirty_mob_effects()
    }

    fn pack_all_equipment(&self) -> Vec<EquipmentSlotItem> {
        equipment_items_to_packet_items(self.inventory.lock().non_empty_equipment_items())
    }

    fn drain_dirty_equipment(&self) -> Vec<EquipmentSlotItem> {
        equipment_items_to_packet_items(self.inventory.lock().drain_dirty_equipment_items())
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

    fn movement_emission(&self) -> EntityMovementEmission {
        if self.is_flying() || self.on_ground() && self.is_discrete() {
            EntityMovementEmission::None
        } else {
            EntityMovementEmission::All
        }
    }

    fn on_below_world(&self) {
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

    fn hurt(&self, source: &DamageSource, amount: f32) -> bool {
        // Delegates to Player's inherent hurt method which handles
        // player-specific prechecks before the shared living hurt path.
        Player::hurt(self, source, amount)
    }

    fn change_world(self: Arc<Self>, teleport_transition: &TeleportTransition) {
        let new_world = teleport_transition.target_world.clone();
        let current_position = self.position();
        let current_rotation = self.rotation();
        let current_velocity = self.velocity();
        let position = teleport_transition.resolved_position(current_position);
        let rotation = teleport_transition.resolved_rotation(current_rotation);
        let velocity =
            teleport_transition.resolved_velocity(current_velocity, current_rotation, rotation);
        self.set_portal_cooldown(teleport_transition.portal_cooldown);
        if !teleport_transition.as_passenger {
            self.stop_riding();
        }
        if Arc::ptr_eq(&self.get_world(), &new_world) {
            if let Err(error) = self.teleport_with_velocity_packet(
                position,
                velocity,
                rotation,
                teleport_transition.position,
                teleport_transition.velocity,
                teleport_transition.rotation,
                teleport_transition.relatives,
            ) {
                panic!(
                    "failed to commit same-world portal teleport for player {}: {error}",
                    self.id()
                );
            }
            self.reset_flying_ticks();
        } else {
            self.reset(new_world, ResetReason::WorldChange);
            if !self.spawn_with_velocity_packet(
                position,
                rotation,
                velocity,
                ResetReason::WorldChange,
                teleport_transition.position,
                teleport_transition.rotation,
                teleport_transition.velocity,
                teleport_transition.relatives,
            ) {
                return;
            }
            // Vanilla: PlayerList.sendAllPlayerInfo -> inventoryMenu.sendAllDataToRemote
            self.send_inventory_to_remote();
        }
        self.apply_post_teleport_transition(&teleport_transition.post_transition);
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

    fn can_be_seen_as_enemy(&self) -> bool {
        !self.abilities.lock().invulnerable
            && !self.is_invulnerable()
            && self.can_be_seen_by_anyone()
    }

    fn is_invulnerable_to(&self, source: &DamageSource) -> bool {
        if self.default_is_invulnerable_to(source)
            || enchantment_helper::is_immune_to_damage(self, source)
        {
            return true;
        }

        if let Some(rule) = Self::disabled_damage_game_rule(source) {
            return self.get_world().get_game_rule(rule) != GameRuleValue::Bool(true);
        }

        !self.has_client_loaded()
    }

    fn actually_hurt(&self, source: &DamageSource, amount: f32) {
        Player::actually_hurt(self, source, amount);
    }

    fn hurt_broadcast_chunk(&self) -> ChunkPos {
        *self.last_chunk_pos.lock()
    }

    fn die(&self, source: &DamageSource) {
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
        &self,
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
    use steel_registry::{
        test_support::init_test_registry, vanilla_damage_types, vanilla_game_rules,
    };
    use steel_utils::types::GameType;

    use crate::entity::damage::DamageSource;

    use super::{Player, ResetReason, nullable_game_mode_id};

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
    fn end_credits_respawn_keeps_vanilla_attribute_data_only() {
        assert_eq!(ResetReason::InitialJoin.respawn_data_kept(), 0x00);
        assert_eq!(ResetReason::Respawn.respawn_data_kept(), 0x00);
        assert_eq!(ResetReason::EndCredits.respawn_data_kept(), 0x01);
        assert_eq!(ResetReason::WorldChange.respawn_data_kept(), 0x03);
    }

    #[test]
    fn disabled_damage_game_rule_matches_vanilla_player_damage_gates() {
        init_test_registry();

        let cases = [
            (
                &vanilla_damage_types::DROWN,
                &vanilla_game_rules::DROWNING_DAMAGE,
            ),
            (
                &vanilla_damage_types::FALL,
                &vanilla_game_rules::FALL_DAMAGE,
            ),
            (
                &vanilla_damage_types::LAVA,
                &vanilla_game_rules::FIRE_DAMAGE,
            ),
            (
                &vanilla_damage_types::FREEZE,
                &vanilla_game_rules::FREEZE_DAMAGE,
            ),
        ];

        for (damage_type, rule) in cases {
            let source = DamageSource::environment(damage_type);
            let mapped = Player::disabled_damage_game_rule(&source);
            assert!(mapped.is_some_and(|mapped| mapped.key == rule.key));
        }
    }

    #[test]
    fn disabled_damage_game_rule_ignores_unrelated_damage() {
        init_test_registry();
        let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

        assert!(Player::disabled_damage_game_rule(&source).is_none());
    }

    #[test]
    fn nullable_game_mode_id_matches_vanilla_encoding() {
        assert_eq!(nullable_game_mode_id(None), -1);
        assert_eq!(nullable_game_mode_id(Some(GameType::Creative)), 1);
    }
}
