//! This module contains all things player-related.
mod abilities;
pub mod advancement;
pub mod chat;
pub mod chunk_sender;
/// This module contains the `PlayerConnection` trait that abstracts network connections.
pub mod connection;
mod container_counter;
mod entity_hooks;
mod entity_state;
/// Experience System
pub mod experience;
pub mod food_data;
/// Game mode specific logic for player interactions.
pub mod game_mode;
mod health_sync;
mod item_cooldowns;
mod lifecycle;
mod living_hooks;
pub mod movement;
mod permissions;
pub mod player_data;
pub mod player_data_storage;
pub mod player_inventory;
mod profile;
mod session;
mod sleep;
mod sleep_state;
mod spam_throttler;
pub mod stats_counter;
mod tick_state;
mod title;

use crate::entity::LivingParts as _;
use crate::entity::prelude::*;
use crate::player::player_inventory::Prediction;
pub use abilities::{Abilities, DEFAULT_FLYING_SPEED};
use chat::ChatState;
pub use chat::{LastSeen, LastSeenMessagesValidator, MessageCache};
use connection::NetworkConnection as _;
pub use connection::{ClientInformation, PlayerConnection};
use container_counter::ContainerCounter;
use food_data::{FoodData, food_constants};
use game_mode::{BlockBreakingManager, PlayerGameModeState};
use glam::DVec3;
use health_sync::HealthSyncState;
use item_cooldowns::ItemCooldowns;
use lifecycle::PlayerLifecycleState;
pub use lifecycle::PlayerRespawnConfig;
pub(crate) use lifecycle::ResetReason;
pub use movement::PlayerInput;
use movement::{MovementState, TeleportState};
use permissions::PlayerPermissionState;
pub(crate) use profile::{GAME_PROFILE_CACHE_LIMIT, KnownPlayerNameLookup, lookup_online_profile};
pub use profile::{
    GameProfile, KnownPlayer, KnownPlayers, ProfileLookupError, is_valid_player_name, offline_uuid,
};
pub use session::PlayerSession;
pub(crate) use session::PlayerSessionId;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use sleep_state::PlayerSleepState;
use std::sync::atomic::{AtomicBool, Ordering};

use std::sync::{Arc, LazyLock, Weak};
use std::time::{Duration, Instant};
use steel_math::RAD_TO_DEG_F64;
use steel_protocol::packets::game::{
    CEntityEvent, CHurtAnimation, CPlayerCombatKill, CPlayerLookAt, CRespawn,
    CSetDefaultSpawnPosition, CSetHealth, CSetHeldSlot, CSetPassengers, ClientCommandAction,
    LookAtAnchor, RelativeMovement, SoundSource,
};
use steel_protocol::packets::game::{CLevelEvent, CSetEntityData, CSetExperience};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_data::{EntityPose, HumanoidArm};
use steel_registry::entity_type::EntityDimensions;
use steel_registry::game_rules::GameRuleRef;
use steel_registry::particle_type::{BlockParticleOption, ParticleData};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entity_data::PlayerEntityData;
use steel_registry::vanilla_game_rules::{
    DROWNING_DAMAGE, FALL_DAMAGE, FIRE_DAMAGE, FREEZE_DAMAGE, IMMEDIATE_RESPAWN, KEEP_INVENTORY,
    SHOW_DEATH_MESSAGES,
};
use steel_registry::vanilla_particle_types;
use steel_registry::{
    level_events, sound_events, vanilla_attributes, vanilla_custom_stats, vanilla_damage_type_tags,
    vanilla_entities, vanilla_game_events,
};
use steel_utils::{entity_events::EntityStatus, locks::Shared, translations};
use tick_state::PlayerTickState;
use uuid::Uuid;

use arc_swap::ArcSwap;
use steel_utils::locks::{CheckedMutex, SyncMutex};
use steel_utils::types::{Difficulty, GameType, InteractionHand};
use text_components::resolving::TextResolutor;
use text_components::translation::TranslatedMessage;
use text_components::{
    Modifier as _, TextComponent,
    interactivity::{ClickEvent, HoverEvent, Interactivity},
};
use text_components::{content::Resolvable, custom::CustomData};

use crate::behavior::{
    BlockStateBehaviorExt as _, ITEM_BEHAVIORS, InteractionResult, InventoryTickContext,
    ItemBehavior, apply_use_remainder,
};
use crate::chunk::chunk_request::{ChunkRequestHandle, ChunkRequestState};
use crate::config::RuntimeConfig;
use crate::enchantment_helper;
use crate::entity::LivingEntityRef;
use crate::entity::LivingProps;
use crate::entity::damage::DamageSource;
use crate::entity::entities::{ExperienceOrbEntity, FishingHookEntity};
use crate::entity::{
    ActiveItemUseState, DEATH_DURATION, Entity, EntityAnchor, EntityBase, EntityEventSource,
    EntityMoveError, EntityMovementEmission, Living, LivingEntityBase, MobEffectSyncChange,
    MobEffectSyncPacket, RemovalReason, SharedEntity, apply_entity_look_at, get_kill_credit,
    start_riding_entities,
};
use crate::fluid::get_fluid_state;
use crate::inventory::equipment::{EntityEquipment, EquipmentSlot, EquipmentSlotType};
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::inventory::menu::Menu;
use crate::inventory::menu::kinds::inventory_menu;
use crate::level_data::RespawnData;
use crate::permission::{
    PermissionContext, PermissionExpr, PermissionMetadataSet, PermissionMetadataValue,
    PermissionSet, PermissionState,
};
use crate::physics::MoveResult;
use crate::player::experience::Experience;
use crate::player::player_data::{PersistentEnderPearl, PersistentRootVehicle};
use crate::player::player_inventory::{
    MenuItemDisposition, MenuRemovalStatus, PlayerInventory, PlayerInventorySyncState,
};
use crate::server::{
    PlayerPacketTransition, Server,
    jobs::{JobPoll, ServerJob, ServerJobContext},
};
use crate::world::player_spawn_finder::{
    POLL_TIME_BUDGET, PlayerSpawnSearch, PlayerSpawnSearchPoll,
};
use steel_registry::vanilla_damage_types;

use steel_protocol::packets::{
    common::SCustomPayload,
    game::{CContainerClose, CGameEvent, CSystemChat, GameEventType},
};
use steel_registry::RegistryEntry;
use steel_registry::damage_type::DeathMessageType;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::stat::vanilla_stat_types;
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos, DowncastType, DowncastTypeKey, Identifier, UuidExt as _,
};

use crate::inventory::container::Container;

const HAT_MODEL_PART_MASK: i8 = 0b0100_0000;

const DROP_SPAM_THROTTLER_INCREMENT_STEP: i32 = 20;
const DROP_SPAM_THROTTLER_THRESHOLD: i32 = 1480;

static INTENTIONAL_GAME_DESIGN_STYLE: LazyLock<Interactivity> = LazyLock::new(|| Interactivity {
    click: Some(ClickEvent::open_url(
        "https://bugs.mojang.com/browse/MCPE-28723",
    )),
    hover: Some(HoverEvent::show_text("MCPE-28723")),
    ..Interactivity::new()
});

use crate::chunk::player_chunk_view::PlayerChunkView;
use crate::entity::{EntityProps, living_entity};
use crate::inventory::ender_chest::{PlayerEnderChestContainer, SyncPlayerEnderChest};
use crate::player::advancement::PlayerAdvancement;
use crate::player::chunk_sender::ChunkSender;
use crate::player::stats_counter::StatsCounter;
use crate::portal::{
    PortalTicketTarget, TeleportPostAction, TeleportPostTransition, TeleportTransition,
};
use crate::world::World;

/// A struct representing a player.
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, entity_type = &vanilla_entities::PLAYER)]
pub struct Player {
    /// The player's game profile.
    pub gameprofile: GameProfile,
    /// The player's connection (abstracted for testing).
    pub connection: Arc<PlayerConnection>,
    /// Stable connection session shared by every incarnation of this player.
    pub(crate) session: Arc<PlayerSession>,

    /// The world the player is in.
    pub world: ArcSwap<World>,

    /// Reference to the server (for entity ID generation, etc.).
    pub(crate) server: Weak<Server>,
    /// Runtime configuration shared with the server.
    pub(crate) config: Arc<RuntimeConfig>,

    /// Common entity fields (id, uuid, position, rotation, removal, callback).
    #[entity(base)]
    base: EntityBase,

    /// Client lifecycle flags.
    lifecycle: CheckedMutex<PlayerLifecycleState>,

    /// Movement tracking state
    pub(crate) movement: CheckedMutex<MovementState>,

    /// Synchronized entity data (health, pose, flags, etc.) for network sync.
    #[entity(synced)]
    entity_data: CheckedMutex<PlayerEntityData>,

    /// The last chunk position of the player.
    pub last_chunk_pos: CheckedMutex<ChunkPos>,
    /// The last chunk tracking view of the player.
    pub last_tracking_view: CheckedMutex<Option<PlayerChunkView>>,
    /// The client's settings/information (language, view distance, chat visibility, etc.).
    /// Updated when the client sends `SClientInformation` during config or play phase.
    client_information: CheckedMutex<ClientInformation>,

    /// Current and previous game mode.
    game_modes: CheckedMutex<PlayerGameModeState>,

    /// The player's inventory container (shared with `inventory_menu`).
    pub inventory: Shared<PlayerInventory>,

    /// Logical inventory slots that must be resent directly to this player's client.
    inventory_sync: CheckedMutex<PlayerInventorySyncState>,

    /// The player's ender chest inventory.
    pub ender_chest_inventory: SyncPlayerEnderChest,

    /// Last main-hand stack used for vanilla attack-strength reset checks.
    last_item_in_main_hand: CheckedMutex<ItemStack>,

    /// The player's inventory menu (always open, even when `container_id` is 0).
    inventory_menu: CheckedMutex<Menu>,

    /// The currently open menu (None if player inventory is open).
    /// This is separate from `inventory_menu` which is always present.
    open_menu: CheckedMutex<player_inventory::OpenMenuState>,

    /// Counter for generating container IDs (1-100, wraps around).
    container_counter: CheckedMutex<ContainerCounter>,

    /// Pending server-initiated teleport state (ID, position, timeout).
    teleport_state: CheckedMutex<TeleportState>,
    /// Vanilla item use cooldown groups.
    item_cooldowns: CheckedMutex<ItemCooldowns>,

    /// Local tick and once-per-tick packet state.
    tick_state: CheckedMutex<PlayerTickState>,
    /// Vanilla sleep/wake animation counter.
    sleep_state: CheckedMutex<PlayerSleepState>,
    /// Persisted personal bed or respawn-anchor target.
    respawn_config: CheckedMutex<Option<PlayerRespawnConfig>>,

    /// Player abilities (flight, invulnerability, build permissions, speeds, etc.)
    pub abilities: CheckedMutex<Abilities>,

    /// Block breaking state machine.
    pub block_breaking: CheckedMutex<BlockBreakingManager>,

    /// Shared living-entity runtime fields (attributes, speed, damage/death state).
    /// Vanilla: `LivingEntity` (L230-232) + `Entity.invulnerableTime` (L256).
    #[entity(component)]
    living_base: LivingEntityBase,

    /// Player food/hunger state (food level, saturation, exhaustion).
    pub food_data: CheckedMutex<FoodData>,

    /// Delta-tracking state for `CSetHealth` deduplication.
    health_sync: CheckedMutex<HealthSyncState>,

    /// The Player's Experience
    pub experience: CheckedMutex<Experience>,

    /// Assigned groups, direct overrides, and the effective permission set.
    permissions: CheckedMutex<PlayerPermissionState>,

    /// Whether the player has completed the vanilla End credits flow.
    seen_credits: AtomicBool,
    /// Set by a mace smash on a grounded target; the next landing sends a block burst.
    spawn_extra_particles_on_fall: AtomicBool,

    /// Vanilla `ServerPlayer.wonGame`; transient while the End credits screen is open.
    won_game: AtomicBool,

    /// Monotonic counter bumped on world teleport/reset. The chunk sending tick
    /// snapshots this before encoding and compares after to detect stale batches.
    pub chunk_send_epoch: CheckedMutex<u32>,

    /// Domain-residence identity and persisted entities awaiting restoration.
    residence: CheckedMutex<PlayerResidenceState>,
    /// In-flight ender pearls thrown by this player, kept weakly so they persist
    /// with the player and re-spawn on login (vanilla `ServerPlayer.enderPearls`).
    ender_pearls: CheckedMutex<Vec<Weak<dyn Entity>>>,

    /// Active fishing hook, kept weakly because the world owns live entities.
    fishing: CheckedMutex<Option<Weak<FishingHookEntity>>>,
    /// The counter keeping track of this player's statistics.
    stats: CheckedMutex<StatsCounter>,

    /// The last action time of this player.
    last_action_time: CheckedMutex<Instant>,

    /// Manage the advancements of the player
    pub advancements: CheckedMutex<PlayerAdvancement>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `Player`.
unsafe impl DowncastType for Player {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/player");
}

#[derive(Clone)]
struct PendingRootVehicleRestore {
    world: Identifier,
    root_vehicle: PersistentRootVehicle,
}

/// Runtime identity for one continuous stay in a Steel domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DomainResidenceToken(u64);

#[derive(Clone)]
struct PlayerResidenceState {
    token: DomainResidenceToken,
    pending_root_vehicle: Option<PendingRootVehicleRestore>,
    pending_ender_pearls: Vec<PersistentEnderPearl>,
}

impl PlayerResidenceState {
    const fn new() -> Self {
        Self {
            token: DomainResidenceToken(1),
            pending_root_vehicle: None,
            pending_ender_pearls: Vec::new(),
        }
    }

    fn advance(&mut self) -> DomainResidenceToken {
        let Some(next_token) = self.token.0.checked_add(1) else {
            panic!("domain residence token space exhausted");
        };
        self.token = DomainResidenceToken(next_token);
        self.pending_root_vehicle = None;
        self.pending_ender_pearls.clear();
        self.token
    }
}

impl Player {
    const USING_ITEM_FLAG: i8 = 1;
    const OFF_HAND_ACTIVE_ITEM_FLAG: i8 = 1 << 1;

    /// Returns the chunk sender owned by this player's connection session.
    pub(crate) fn chunk_sender(&self) -> &SyncMutex<ChunkSender> {
        &self.session.chunk_sender
    }

    /// Returns chat protocol state owned by this connection session.
    pub(crate) fn chat(&self) -> &SyncMutex<ChatState> {
        &self.session.chat
    }

    /// Returns the hand currently driving active item use.
    #[must_use]
    pub fn active_item_use_hand(&self) -> Option<InteractionHand> {
        self.living_base
            .active_item_use()
            .map(|active| active.hand())
    }

    /// Starts using the item currently held in `hand`.
    pub fn start_using_item(&self, hand: InteractionHand) {
        let item = {
            let inventory = self.inventory.lock();
            let item = inventory.get_item_in_hand(hand);
            item.clone()
        };
        let duration = ITEM_BEHAVIORS
            .get_behavior(item.item())
            .get_use_duration(&item, &self.living());
        if self.living_base.start_using_item(hand, &item, duration) {
            let mut entity_data = self.entity_data.lock();
            let flags = entity_data.living_entity().living_entity_flags.get();
            let mut flags = *flags | Self::USING_ITEM_FLAG;
            if hand == InteractionHand::OffHand {
                flags |= Self::OFF_HAND_ACTIVE_ITEM_FLAG;
            } else {
                flags &= !Self::OFF_HAND_ACTIVE_ITEM_FLAG;
            }
            entity_data
                .living_entity_mut()
                .living_entity_flags
                .set(flags);
        }
    }

    fn stop_using_item(&self) {
        self.living_base.stop_using_item();
        let mut entity_data = self.entity_data.lock();
        let flags = *entity_data.living_entity().living_entity_flags.get();
        entity_data
            .living_entity_mut()
            .living_entity_flags
            .set(flags & !Self::USING_ITEM_FLAG);
    }

    /// Releases the currently used item and invokes its release hook.
    pub fn release_using_item(&self, entity: &SharedEntity) {
        let Some(active) = self.living_base.active_item_use() else {
            return;
        };
        let behavior = ITEM_BEHAVIORS.get_behavior(active.item());
        self.release_using_item_with_behavior(entity, behavior, active);
    }

    fn release_using_item_with_behavior(
        &self,
        entity: &SharedEntity,
        behavior: &dyn ItemBehavior,
        active: ActiveItemUseState,
    ) {
        let hand = active.hand();
        let item_matches = {
            let inventory = self.inventory.lock();
            inventory.get_item_in_hand(hand).item() == active.item()
        };
        if !item_matches {
            self.stop_using_item();
            return;
        }
        // Read a copy rather than clearing the slot,
        // so any inventory-touching side effect from
        // `release_using` never sees the hand as vacant.
        let mut item = {
            let inventory = self.inventory.lock();
            let current = inventory.get_item_in_hand(hand);
            current.clone()
        };
        let stack_before_using = item.clone();
        let world = self.get_world();
        let apply_side_effects =
            behavior.release_using(&mut item, &world, &self.living(), active.remaining_ticks());
        let use_on_release = behavior.use_on_release(&item);
        if apply_side_effects {
            item = apply_use_remainder(&stack_before_using, item, &self.living());
            self.apply_item_use_cooldown(&stack_before_using);
        }
        self.inventory.lock().set_item_in_hand(hand, item);
        // we re-read active here since behavior.release_using might have already ended the use
        if use_on_release && let Some(active) = self.living_base.active_item_use() {
            self.tick_active_item_use_with_behavior(entity, behavior, active);
        }
        self.stop_using_item();
    }

    fn tick_active_item_use(&self, entity: &SharedEntity) {
        let Some(active) = self.living_base.active_item_use() else {
            return;
        };
        let behavior = ITEM_BEHAVIORS.get_behavior(active.item());
        self.tick_active_item_use_with_behavior(entity, behavior, active);
    }

    fn tick_active_item_use_with_behavior(
        &self,
        entity: &SharedEntity,
        behavior: &dyn ItemBehavior,
        active: ActiveItemUseState,
    ) {
        let hand = active.hand();
        let item_matches = {
            let inventory = self.inventory.lock();
            inventory.get_item_in_hand(hand).item() == active.item()
        };
        if !item_matches {
            self.stop_using_item();
            return;
        }
        let mut item = {
            let inventory = self.inventory.lock();
            let current = inventory.get_item_in_hand(hand);
            current.clone()
        };
        let world = self.get_world();
        let Some(user) = LivingEntityRef::new(entity) else {
            panic!("active item use requires a living entity");
        };
        behavior.on_use_tick(&world, user, &mut item, active.remaining_ticks());

        if self.active_item_use_hand() != Some(hand) {
            self.inventory.lock().set_item_in_hand(hand, item);
            return;
        }
        let Some(active) = self.living_base.decrement_active_item_use() else {
            self.inventory.lock().set_item_in_hand(hand, item);
            return;
        };
        let use_on_release = behavior.use_on_release(&item);
        if active.remaining_ticks() == 0 && !use_on_release && !item.is_empty() {
            self.send_packet(CEntityEvent {
                entity_id: self.id(),
                event: EntityStatus::UseItemComplete,
            });
            let stack_before_finish = item.clone();
            item = behavior.finish_using(&mut item, &world, user);
            self.apply_item_use_cooldown(&stack_before_finish);
            self.stop_using_item();
        }

        self.inventory.lock().set_item_in_hand(hand, item);
    }

    /// Returns the player's configured main arm.
    #[must_use]
    pub fn main_arm(&self) -> HumanoidArm {
        self.client_information.lock().main_hand
    }

    #[must_use]
    pub(crate) fn shows_hat(&self) -> bool {
        let model_customization = *self
            .entity_data
            .lock()
            .avatar()
            .player_mode_customization
            .get();
        model_customization & HAT_MODEL_PART_MASK != 0
    }

    fn apply_client_information_to_entity_data(
        data: &mut PlayerEntityData,
        client_information: &ClientInformation,
    ) {
        let avatar = data.avatar_mut();
        avatar.player_main_hand.set(client_information.main_hand);
        avatar
            .player_mode_customization
            .set(client_information.model_customization.cast_signed());
    }

    /// Computes the start (eye position) and end positions for a raytrace.
    pub fn get_ray_endpoints(&self) -> (DVec3, DVec3) {
        let pos = self.position();
        let start_pos = DVec3::new(pos.x, self.eye_y(), pos.z);
        let block_interaction_range = self.block_interaction_range();
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
    #[expect(
        clippy::too_many_arguments,
        reason = "player construction requires explicit connection, session, world, and identity owners"
    )]
    pub fn new(
        gameprofile: GameProfile,
        connection: Arc<PlayerConnection>,
        session: Arc<PlayerSession>,
        world: Arc<World>,
        server: Weak<Server>,
        config: Arc<RuntimeConfig>,
        entity_id: i32,
        client_information: ClientInformation,
    ) -> Self {
        // Create a single shared inventory container used by both the player and inventory menu
        let inventory = Arc::new(SyncMutex::new(PlayerInventory::new()));
        let ender_chest_inventory = Arc::new(SyncMutex::new(PlayerEnderChestContainer::new()));

        let pos = DVec3::new(0.0, 0.0, 0.0);

        let equipment = Arc::clone(&inventory);
        let mut entity_data = PlayerEntityData::new();
        let living_base = LivingEntityBase::with_equipment(
            &vanilla_entities::PLAYER,
            equipment,
            &Self::LIVING,
            &mut entity_data,
        );
        Self::apply_client_information_to_entity_data(&mut entity_data, &client_information);
        let player_uuid = gameprofile.id();
        let world_ref = Arc::downgrade(&world);
        Self {
            gameprofile,
            connection,
            session,

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
            lifecycle: CheckedMutex::new(PlayerLifecycleState::default()),
            movement: CheckedMutex::new(MovementState::new()),
            entity_data: CheckedMutex::new(entity_data),
            last_chunk_pos: CheckedMutex::new(ChunkPos::new(0, 0)),
            last_tracking_view: CheckedMutex::new(None),
            client_information: CheckedMutex::new(client_information),
            game_modes: CheckedMutex::new(PlayerGameModeState::new(GameType::Survival)),
            inventory: Arc::clone(&inventory),
            inventory_sync: CheckedMutex::new(PlayerInventorySyncState::new()),
            ender_chest_inventory,
            last_item_in_main_hand: CheckedMutex::new(ItemStack::empty()),
            inventory_menu: CheckedMutex::new(inventory_menu(inventory)),
            open_menu: CheckedMutex::new(player_inventory::OpenMenuState::new()),
            container_counter: CheckedMutex::new(ContainerCounter::new()),
            teleport_state: CheckedMutex::new(TeleportState::new()),
            item_cooldowns: CheckedMutex::new(ItemCooldowns::default()),
            tick_state: CheckedMutex::new(PlayerTickState::new()),
            sleep_state: CheckedMutex::new(PlayerSleepState::new()),
            respawn_config: CheckedMutex::new(None),
            abilities: CheckedMutex::new(Abilities::default()),
            block_breaking: CheckedMutex::new(BlockBreakingManager::new()),
            living_base,
            food_data: CheckedMutex::new(FoodData::new()),
            health_sync: CheckedMutex::new(HealthSyncState::new()),
            experience: CheckedMutex::new(Experience::default()),
            permissions: CheckedMutex::new(PlayerPermissionState::default()),
            seen_credits: AtomicBool::new(false),
            spawn_extra_particles_on_fall: AtomicBool::new(false),
            won_game: AtomicBool::new(false),
            chunk_send_epoch: CheckedMutex::new(0),
            residence: CheckedMutex::new(PlayerResidenceState::new()),
            ender_pearls: CheckedMutex::new(Vec::new()),
            fishing: CheckedMutex::new(None),
            stats: CheckedMutex::new(StatsCounter::new()),
            last_action_time: CheckedMutex::new(Instant::now()),
            advancements: CheckedMutex::new(PlayerAdvancement::default()), // TODO save the advancement
        }
    }

    fn send_experience_packet_if_dirty(&self) {
        let experience_packet = {
            let mut experience = self.experience.lock();
            if experience.dirty {
                experience.dirty = false;
                Some(CSetExperience {
                    progress: experience.progress(),
                    level: experience.level(),
                    total_experience: experience.total_points(),
                })
            } else {
                None
            }
        };
        if let Some(packet) = experience_packet {
            self.send_packet(packet);
        }
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
            world.chunk_map.remove_player(self);
            world.entity_tracker().on_player_leave(self);
            world.player_area_map.remove_by_entity_id(self.id());
            self.set_removed(RemovalReason::Killed);
            assert_eq!(
                self.remove_all_menus_with_disposition(MenuItemDisposition::Drop),
                MenuRemovalStatus::Complete,
                "death removal menu cleanup must run outside a menu callback"
            );
        }
    }

    /// Ticks to award stats that are awarded based on ticks.
    fn tick_stats(&self) {
        // These stats are expressed in ticks.
        // We batch the stats so that the mutex for stats is only
        // locked once.
        let mut stats = self.stats.lock();
        stats.increment(
            vanilla_stat_types::CUSTOM.get(&vanilla_custom_stats::PLAY_TIME),
            1,
        );
        stats.increment(
            vanilla_stat_types::CUSTOM.get(&vanilla_custom_stats::TOTAL_WORLD_TIME),
            1,
        );

        if Entity::is_alive(self) {
            stats.increment(
                vanilla_stat_types::CUSTOM.get(&vanilla_custom_stats::TIME_SINCE_DEATH),
                1,
            );
        }
        if self.is_discrete() {
            stats.increment(
                vanilla_stat_types::CUSTOM.get(&vanilla_custom_stats::SNEAK_TIME),
                1,
            );
        }
        if !self.living().is_sleeping() {
            stats.increment(
                vanilla_stat_types::CUSTOM.get(&vanilla_custom_stats::TIME_SINCE_REST),
                1,
            );
        }
    }

    /// Immediately flushes dirty player entity data to tracking players and self.
    fn sync_entity_data(&self) {
        let dirty_values = self.entity_data.lock().pack_dirty();
        if let Some(dirty_values) = dirty_values {
            let packet = CSetEntityData::new(self.id(), dirty_values);
            self.get_world()
                .broadcast_to_entity_trackers(self.id(), packet.clone(), None);
            self.send_packet(packet);
        }
    }

    /// Handles a custom payload packet.
    #[expect(clippy::unused_self, reason = "this is an api function")]
    pub fn handle_custom_payload(&self, _packet: SCustomPayload) {}

    /// Handles the end of a client tick.
    pub fn handle_client_tick_end(&self) {
        self.movement.lock().finish_client_tick();
    }

    /// Main entry point for dealing damage. Returns `true` if damage was applied.
    ///
    /// `world` is vanilla's explicit `ServerLevel` argument and controls
    /// difficulty scaling and damage gamerules.
    pub fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        if self.living().is_invulnerable_to(world, source) {
            return false;
        }

        {
            let abilities = self.abilities.lock();
            if abilities.invulnerable && !source.bypasses_invulnerability() {
                return false;
            }
        }

        // TODO: reset player noActionTime and remove shoulder entities.
        if self.living().health() <= 0.0 {
            return false;
        }

        // Difficulty scaling (vanilla: Player.hurtServer)
        let mut amount = amount;
        if source.scales_with_difficulty() {
            let difficulty = world.level_data.read().data().difficulty;
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

        self.living().hurt_server(world, source, amount)
    }

    fn disabled_damage_game_rule(source: &DamageSource) -> Option<GameRuleRef<bool>> {
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

    /// Applies vanilla player damage reductions and health loss.
    fn actually_hurt(&self, world: &World, source: &DamageSource, amount: f32) {
        if self.living().is_invulnerable_to(world, source) {
            return;
        }

        let damage = self.living().damage_after_armor_absorb(source, amount);
        let damage = self.living().damage_after_magic_absorb(source, damage);
        let original_damage = damage;
        let damage = (damage - self.living().absorption_amount()).max(0.0);
        self.living()
            .set_absorption_amount(self.living().absorption_amount() - (original_damage - damage));

        let absorbed_damage = original_damage - damage;
        if (0.0..f32::MAX).contains(&absorbed_damage) {
            self.award_custom_stat_with_count(
                &vanilla_custom_stats::DAMAGE_ABSORBED,
                (absorbed_damage * 10.0).round() as i32,
            );
        }

        // TODO: combat tracker (getCombatTracker().recordDamage)
        if damage != 0.0 {
            self.cause_food_exhaustion(source.damage_type.exhaustion);
            self.living().set_health(self.living().health() - damage);
            if damage < f32::MAX {
                self.award_custom_stat_with_count(
                    &vanilla_custom_stats::DAMAGE_TAKEN,
                    (damage * 10.0).round() as i32,
                );
            }
            self.game_event(&vanilla_game_events::ENTITY_DAMAGE);
        }
    }

    fn get_death_message(&self, source: &DamageSource) -> TextComponent {
        let death_key = format!("death.attack.{}", source.damage_type.message_id);
        match source.damage_type.death_message_type {
            DeathMessageType::IntentionalGameDesign => {
                let mut link = translations::CHAT_SQUARE_BRACKETS
                    .message([TranslatedMessage {
                        key: format!("{death_key}.link").into(),
                        fallback: None,
                        args: None,
                    }
                    .component()])
                    .component();
                link.interactions = INTENTIONAL_GAME_DESIGN_STYLE.clone();

                TranslatedMessage {
                    key: format!("{death_key}.message").into(),
                    fallback: None,
                    args: Some(Box::new([self.display_name(), link])),
                }
                .component()
            }
            DeathMessageType::Default | DeathMessageType::FallVariants => {
                // TODO: use CombatTracker for killer name items and fall variants
                TranslatedMessage {
                    key: death_key.into(),
                    fallback: None,
                    args: Some(Box::new([TextComponent::plain(
                        self.gameprofile.name().to_owned(),
                    )])),
                }
                .component()
            }
        }
    }

    /// Vanilla: `ServerPlayer.die()` (does NOT call `super.die()`).
    fn die(&self, source: &DamageSource) {
        if self.is_removed() {
            return;
        }
        if !self.living_base.mark_death_processed() {
            return;
        }

        self.game_event(&vanilla_game_events::ENTITY_DIE);

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

        let show_death_messages = world.get_game_rule(&SHOW_DEATH_MESSAGES);

        let death_message = self.get_death_message(source);

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

        if !world.get_game_rule(&KEEP_INVENTORY) && self.game_mode() != GameType::Spectator {
            let drops = self.inventory.lock().take_death_drops();
            for item in drops {
                let _ = self.spawn_dropped_item(item, true, false);
            }

            let reward = self.experience.lock().death_xp_reward();
            if reward > 0 {
                ExperienceOrbEntity::award(&world, self.position(), reward);
            }
        }

        // TODO: Increment DEATH_COUNT objective criterion
        if let Some(killer) = get_kill_credit(&self.living(), world.as_ref()) {
            self.award_stat(&vanilla_stat_types::ENTITY_KILLED_BY, killer.entity_type());
            killer.award_kill_score(self, source);
            // TODO: Create wither rose
        }

        self.award_custom_stat(&vanilla_custom_stats::DEATHS);
        self.reset_custom_stat(&vanilla_custom_stats::TIME_SINCE_DEATH);
        self.reset_custom_stat(&vanilla_custom_stats::TIME_SINCE_REST);

        self.clear_fire();
        self.set_ticks_frozen(0);

        if world.get_game_rule(&IMMEDIATE_RESPAWN) {
            self.respawn();
        }
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

    /// Returns the identity of the player's current continuous domain stay.
    pub(crate) fn domain_residence_token(&self) -> DomainResidenceToken {
        self.residence.lock().token
    }

    /// Starts a new continuous domain stay and invalidates old restore work.
    pub(crate) fn advance_domain_residence(&self) -> DomainResidenceToken {
        self.residence.lock().advance()
    }

    /// Returns whether delayed work still belongs to the current domain stay.
    pub(crate) fn is_domain_residence_current(&self, token: DomainResidenceToken) -> bool {
        self.residence.lock().token == token
    }

    /// Installs both persisted restore payloads for a token-owned domain stay.
    pub(crate) fn install_pending_domain_restores(
        &self,
        token: DomainResidenceToken,
        world: &World,
        root_vehicle: Option<PersistentRootVehicle>,
        ender_pearls: Vec<PersistentEnderPearl>,
    ) -> bool {
        let mut residence = self.residence.lock();
        if residence.token != token {
            return false;
        }

        residence.pending_root_vehicle =
            root_vehicle.map(|root_vehicle| PendingRootVehicleRestore {
                world: world.key.clone(),
                root_vehicle,
            });
        residence.pending_ender_pearls = ender_pearls;
        true
    }

    pub(crate) fn pending_root_vehicle_for_current_world(&self) -> Option<PersistentRootVehicle> {
        let world_key = self.get_world().key.clone();
        self.residence
            .lock()
            .pending_root_vehicle
            .as_ref()
            .filter(|pending| pending.world == world_key)
            .map(|pending| pending.root_vehicle.clone())
    }

    pub(crate) fn take_matching_pending_root_vehicle(
        &self,
        token: DomainResidenceToken,
        world: &World,
        attach: [u8; 16],
        root_uuid: [u8; 16],
    ) -> Option<PersistentRootVehicle> {
        let mut residence = self.residence.lock();
        if residence.token != token {
            return None;
        }
        let matches = residence
            .pending_root_vehicle
            .as_ref()
            .is_some_and(|pending| {
                pending.world == world.key
                    && pending.root_vehicle.attach == attach
                    && pending.root_vehicle.entity.uuid == root_uuid
            });
        if matches {
            residence
                .pending_root_vehicle
                .take()
                .map(|pending| pending.root_vehicle)
        } else {
            None
        }
    }

    pub(crate) fn pending_ender_pearls(&self) -> Vec<PersistentEnderPearl> {
        self.residence.lock().pending_ender_pearls.clone()
    }

    pub(crate) fn remove_pending_ender_pearl(&self, uuid: Uuid) {
        self.residence
            .lock()
            .pending_ender_pearls
            .retain(|pearl| Uuid::from_bytes(pearl.entity.uuid) != uuid);
    }

    pub(crate) fn discard_pending_ender_pearl(
        &self,
        token: DomainResidenceToken,
        uuid: Uuid,
    ) -> bool {
        let mut residence = self.residence.lock();
        if residence.token != token {
            return false;
        }
        let old_len = residence.pending_ender_pearls.len();
        residence
            .pending_ender_pearls
            .retain(|pearl| Uuid::from_bytes(pearl.entity.uuid) != uuid);
        residence.pending_ender_pearls.len() != old_len
    }

    pub(crate) fn take_matching_pending_ender_pearl(
        &self,
        token: DomainResidenceToken,
        world: &World,
        uuid: Uuid,
    ) -> Option<PersistentEnderPearl> {
        let mut residence = self.residence.lock();
        if residence.token != token {
            return None;
        }
        let world_key = world.key.to_string();
        let index = residence.pending_ender_pearls.iter().position(|pearl| {
            pearl.world == world_key && Uuid::from_bytes(pearl.entity.uuid) == uuid
        })?;
        Some(residence.pending_ender_pearls.remove(index))
    }

    /// The hook this player is fishing with (vanilla `Player.fishing`).
    /// A removed hook no longer counts, as vanilla clears the field on removal.
    pub fn fishing_hook(&self) -> Option<Arc<FishingHookEntity>> {
        let mut fishing = self.fishing.lock();
        let hook = fishing
            .as_ref()
            .and_then(Weak::upgrade)
            .filter(|hook| !hook.is_removed());
        if hook.is_none() {
            *fishing = None;
        }
        hook
    }

    /// Records the hook this player now fishes with.
    pub fn set_fishing_hook(&self, hook: &Arc<FishingHookEntity>) {
        *self.fishing.lock() = Some(Arc::downgrade(hook));
    }

    /// Registers a thrown ender pearl so it persists with this player and
    /// re-spawns on login (vanilla `ServerPlayer.registerEnderPearl`).
    pub fn register_ender_pearl(&self, pearl: &SharedEntity) {
        let uuid = pearl.uuid();
        let mut pearls = self.ender_pearls.lock();
        pearls.retain(|weak| {
            weak.upgrade()
                .is_some_and(|p| !p.is_removed() && p.uuid() != uuid)
        });
        pearls.push(Arc::downgrade(pearl));
        drop(pearls);
        self.remove_pending_ender_pearl(uuid);
    }

    /// Deregisters a thrown ender pearl once it hits, teleports, or is discarded
    /// (vanilla `ServerPlayer.deregisterEnderPearl`).
    pub fn deregister_ender_pearl(&self, uuid: Uuid) {
        self.ender_pearls
            .lock()
            .retain(|weak| weak.upgrade().is_some_and(|p| p.uuid() != uuid));
    }

    /// Returns this player's live, in-flight ender pearls, pruning dead entries.
    #[must_use]
    pub fn ender_pearls(&self) -> Vec<SharedEntity> {
        let mut pearls = self.ender_pearls.lock();
        pearls.retain(|weak| weak.upgrade().is_some_and(|p| !p.is_removed()));
        pearls.iter().filter_map(Weak::upgrade).collect()
    }

    /// Rebinds live pearls to a fresh respawn incarnation with the same player UUID.
    pub(crate) fn rebind_ender_pearls_to(&self, replacement: &Arc<Self>) {
        debug_assert_eq!(self.gameprofile.id(), replacement.gameprofile.id());
        let replacement_entity: SharedEntity = Arc::<Player>::clone(replacement);
        for pearl in self.ender_pearls() {
            if pearl.projectile_owner_uuid() == Some(self.gameprofile.id()) {
                pearl.restore_owner_reference(&replacement_entity);
            }
        }
    }

    /// Appends vanilla-shaped player state used by command NBT predicates.
    pub(crate) fn save_command_nbt(&self, nbt: &mut NbtCompound) {
        {
            let inventory = self.inventory.lock();
            nbt.insert("Inventory", inventory.to_vanilla_inventory_nbt());
            nbt.insert("SelectedItemSlot", i32::from(inventory.get_selected_slot()));
        }

        {
            let experience = self.experience.lock();
            nbt.insert("XpP", experience.progress());
            nbt.insert("XpLevel", experience.level());
            nbt.insert("XpTotal", experience.total_points());
            nbt.insert("XpSeed", experience.enchantment_seed());
        }
        nbt.insert("Score", self.score());

        {
            let food = self.food_data.lock();
            nbt.insert("foodLevel", food.food_level);
            nbt.insert("foodTickTimer", food.tick_timer);
            nbt.insert("foodSaturationLevel", food.saturation_level);
            nbt.insert("foodExhaustionLevel", food.exhaustion_level);
        }

        {
            let abilities = self.abilities.lock();
            let mut abilities_nbt = NbtCompound::new();
            abilities_nbt.insert(
                "invulnerable",
                NbtTag::Byte(i8::from(abilities.invulnerable)),
            );
            abilities_nbt.insert("flying", NbtTag::Byte(i8::from(abilities.flying)));
            abilities_nbt.insert("mayfly", NbtTag::Byte(i8::from(abilities.may_fly)));
            abilities_nbt.insert("instabuild", NbtTag::Byte(i8::from(abilities.instabuild)));
            abilities_nbt.insert("mayBuild", NbtTag::Byte(i8::from(abilities.may_build)));
            abilities_nbt.insert("flySpeed", abilities.flying_speed);
            abilities_nbt.insert("walkSpeed", abilities.walking_speed);
            nbt.insert("abilities", NbtTag::Compound(abilities_nbt));
        }

        nbt.insert("playerGameType", self.game_mode() as i32);
        if let Some(previous_game_mode) = self.previous_game_mode() {
            nbt.insert("previousPlayerGameType", previous_game_mode as i32);
        }
        nbt.insert(
            "seenCredits",
            NbtTag::Byte(i8::from(self.has_seen_credits())),
        );
        nbt.insert("Dimension", self.get_world().key.to_string());

        if let Some(vehicle) = self.vehicle()
            && let Some(root_vehicle) = self.root_vehicle()
            && root_vehicle.id() != self.id()
            && root_vehicle.has_exactly_one_player_passenger()
            && let Some(entity_nbt) = root_vehicle.nbt_for_passenger_save()
        {
            let mut root_vehicle_nbt = NbtCompound::new();
            root_vehicle_nbt.insert(
                "Attach",
                NbtTag::IntArray(vehicle.uuid().to_int_array().to_vec()),
            );
            root_vehicle_nbt.insert("Entity", NbtTag::Compound(entity_nbt));
            nbt.insert("RootVehicle", NbtTag::Compound(root_vehicle_nbt));
        }

        let ender_pearls = self
            .ender_pearls()
            .into_iter()
            .filter_map(|pearl| {
                let world = pearl.level()?;
                let mut pearl_nbt = pearl.nbt_for_passenger_save()?;
                pearl_nbt.insert("ender_pearl_dimension", world.key.to_string());
                Some(pearl_nbt)
            })
            .collect::<Vec<_>>();
        if !ender_pearls.is_empty() {
            nbt.insert("ender_pearls", NbtList::Compound(ender_pearls));
        }
    }

    /// Marks live ender pearls as stored with this player so chunk saves remove
    /// them from world storage and player data remains the sole owner.
    pub fn store_ender_pearls_with_player(&self) {
        for pearl in self.ender_pearls() {
            let world = pearl.level();
            let chunk = ChunkPos::from_entity_pos(pearl.position());
            pearl.set_removed(RemovalReason::StoredWithPlayer);
            if let Some(world) = world {
                world.mark_chunk_dirty(chunk);
            }
        }
    }

    /// Returns this player's local server tick count.
    #[must_use]
    pub fn tick_count(&self) -> i32 {
        self.tick_state.lock().tick_count()
    }

    #[must_use]
    pub(crate) fn take_xp_delay(&self) -> i32 {
        self.tick_state.lock().take_xp_delay()
    }

    pub(crate) fn set_take_xp_delay(&self, delay: i32) {
        self.tick_state.lock().set_take_xp_delay(delay);
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

    /// Resets the last action time of the player (to the current time).
    pub fn reset_last_action_time(&self) {
        *self.last_action_time.lock() = Instant::now();
    }

    fn check_idle_timeout(&self) {
        if let Some(server) = self.server.upgrade() {
            let player_idle_timeout = server.player_idle_timeout.load(Ordering::Relaxed);
            if player_idle_timeout > 0
                && Instant::now().duration_since(*self.last_action_time.lock())
                    > Duration::from_mins(player_idle_timeout as u64)
                && !self.has_won_game()
            {
                self.disconnect(translations::MULTIPLAYER_DISCONNECT_IDLING.msg());
            }
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
mod tests;
