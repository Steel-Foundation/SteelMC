//! This module contains all things player-related.
mod abilities;
pub mod block_breaking;
mod chat_state;
pub mod chunk_sender;
/// This module contains the `PlayerConnection` trait that abstracts network connections.
pub mod connection;
mod entity_state;
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
use health_sync::HealthSyncState;
pub use message_validator::LastSeenMessagesValidator;
use movement_state::MovementState;
pub use signature_cache::{LastSeen, MessageCache};
use steel_protocol::packet_traits::CompressionInfo;
use teleport_state::TeleportState;

use block_breaking::BlockBreakingManager;
use crossbeam::atomic::AtomicCell;
use enum_dispatch::enum_dispatch;
pub use game_profile::{GameProfile, GameProfileAction};
use message_chain::SignedMessageChain;
use profile_key::RemoteChatSession;
use std::{
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use steel_protocol::packet_traits::{ClientPacket, EncodedPacket};
use steel_protocol::packets::game::CSystemChatMessage;
use steel_protocol::packets::game::{
    AnimateAction, CAddEntity, CAnimate, CDamageEvent, CEntityEvent, CEntityPositionSync,
    CHurtAnimation, COpenSignEditor, CPlayerCombatKill, CPlayerPosition, CRemoveEntities, CRespawn,
    CSetEntityData, CSetHealth, CSetHeldSlot, CSetTime, ClientCommandAction, PlayerAction,
    SAcceptTeleportation, SPickItemFromBlock, SPlayerAbilities, SPlayerAction, SSetCarriedItem,
    SUseItem, SUseItemOn,
};
use steel_protocol::utils::ConnectionProtocol;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::shapes::AABBd;
use steel_registry::entity_data::EntityPose;
use steel_registry::entity_types::EntityTypeRef;
use steel_registry::game_rules::GameRuleValue;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::PlayerEntityData;
use steel_registry::vanilla_game_rules::{
    ADVANCE_TIME, ELYTRA_MOVEMENT_CHECK, IMMEDIATE_RESPAWN, KEEP_INVENTORY, PLAYER_MOVEMENT_CHECK,
    SHOW_DEATH_MESSAGES,
};
use steel_registry::{REGISTRY, vanilla_chat_types};
use steel_utils::entity_events::EntityStatus;

use steel_utils::locks::SyncMutex;
use steel_utils::types::GameType;
use text_components::resolving::TextResolutor;
use text_components::translation::TranslatedMessage;
use text_components::{Modifier, TextComponent};
use text_components::{
    content::Resolvable,
    custom::CustomData,
    interactivity::{ClickEvent, HoverEvent},
};
use uuid::Uuid;

use crate::entity::{DEATH_DURATION, LivingEntityBase};
use crate::player::player_inventory::PlayerInventory;
use crate::server::Server;
use crate::{command::commands::gamemode::get_gamemode_translation, inventory::SyncPlayerInv};
use crate::{
    config::STEEL_CONFIG,
    entity::{Entity, EntityLevelCallback, NullEntityCallback, RemovalReason},
};
use crate::{config::WorldGeneratorTypes, entity::damage::DamageSource};
use steel_registry::vanilla_damage_types;

use steel_crypto::{SignatureValidator, public_key_from_bytes, signature::NoValidation};
use steel_protocol::packets::{
    common::{SClientInformation, SCustomPayload},
    game::{
        CBlockChangedAck, CBlockUpdate, CContainerClose, CGameEvent, CMoveEntityPosRot,
        CMoveEntityRot, COpenScreen, CPlayerChat, CPlayerInfoUpdate, CRotateHead,
        CSetChunkCacheRadius, CSystemChat, ChatTypeBound, FilterType, GameEventType,
        PreviousMessage, SChat, SChatAck, SChatSessionUpdate, SContainerButtonClick,
        SContainerClick, SContainerClose, SContainerSlotStateChanged, SMovePlayer, SPlayerInput,
        SSetCreativeModeSlot, SSignUpdate, calc_delta, to_angle_byte,
    },
};
use steel_registry::{blocks::properties::Direction, item_stack::ItemStack};

use crate::behavior::{BLOCK_BEHAVIORS, InteractionResult};
use crate::block_entity::BlockEntity;
use crate::block_entity::entities::SignBlockEntity;
use steel_utils::BlockPos;

use steel_utils::types::InteractionHand;
use steel_utils::{ChunkPos, math::Vector3, translations};

use crate::entity::LivingEntity;
use crate::inventory::{
    MenuInstance, MenuProvider,
    container::Container,
    inventory_menu::InventoryMenu,
    lock::{ContainerId, ContainerLockGuard},
    menu::Menu,
    slot::Slot,
};

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
    pub model_customisation: i32,
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
            model_customisation: 0,
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
use crate::world::World;

/// A struct representing a player.
pub struct Player {
    /// The player's game profile.
    pub gameprofile: GameProfile,
    /// The player's connection (abstracted for testing).
    pub connection: Arc<PlayerConnection>,

    /// The world the player is in.
    pub world: Arc<World>,

    /// Reference to the server (for entity ID generation, etc.).
    #[allow(dead_code)]
    pub(crate) server: Weak<Server>,

    /// The entity ID assigned to this player.
    pub id: i32,

    /// Whether the player has finished loading the client.
    pub client_loaded: AtomicBool,

    /// The player's position.
    pub position: SyncMutex<Vector3<f64>>,
    /// The player's rotation (yaw, pitch).
    pub rotation: AtomicCell<(f32, f32)>,
    /// Movement tracking state (prev position/rotation, velocity, validation, broadcast sync).
    pub(crate) movement: SyncMutex<MovementState>,

    /// Synchronized entity data (health, pose, flags, etc.) for network sync.
    entity_data: SyncMutex<PlayerEntityData>,

    /// The player's movement speed.
    speed: AtomicCell<f32>,

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

    /// Delta-tracking state for `CSetHealth` deduplication.
    health_sync: SyncMutex<HealthSyncState>,

    /// Whether the player has been removed from the world.
    removed: AtomicBool,

    /// Callback for entity lifecycle events (movement between chunks, removal).
    level_callback: SyncMutex<Arc<dyn EntityLevelCallback>>,
}

impl Player {
    /// Creates a new player.
    pub fn new(
        gameprofile: GameProfile,
        connection: Arc<PlayerConnection>,
        world: Arc<World>,
        server: Weak<Server>,
        entity_id: i32,
        player: &Weak<Player>,
        client_information: ClientInformation,
    ) -> Self {
        // Create a single shared inventory container used by both the player and inventory menu
        let inventory = Arc::new(SyncMutex::new(PlayerInventory::new(player.clone())));

        let pos = Vector3::new(0.0, 0.0, 0.0);

        Self {
            gameprofile,
            connection,

            world,
            server,
            id: entity_id,
            client_loaded: AtomicBool::new(false),
            position: SyncMutex::new(pos),
            rotation: AtomicCell::new((0.0, 0.0)),
            movement: SyncMutex::new(MovementState::new()),
            entity_data: SyncMutex::new({
                let mut data = PlayerEntityData::new();
                data.health.set(20.0);
                data
            }),
            speed: AtomicCell::new(0.1), // Default walking speed
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
            health_sync: SyncMutex::new(HealthSyncState::new()),
            removed: AtomicBool::new(false),
            level_callback: SyncMutex::new(Arc::new(NullEntityCallback)),
        }
    }

    /// Sends a packet to the player's connection.
    ///
    /// This is a generic helper that encodes the packet and delegates to the
    /// connection's `send_encoded` method, enabling object-safe packet sending.
    ///
    /// # Panics
    ///
    /// Panics if the packet fails to encode.
    pub fn send_packet<P: ClientPacket>(&self, packet: P) {
        let encoded = EncodedPacket::from_bare(
            packet,
            self.connection.compression(),
            ConnectionProtocol::Play,
        )
        .expect("Failed to encode packet");
        self.connection.send_encoded(encoded);
    }

    /// Sends multiple packets as an atomic bundle.
    ///
    /// The closure receives a [`BundleBuilder`](networking::BundleBuilder) to add packets to.
    /// All packets are encoded, then sent wrapped in bundle delimiters so the
    /// client processes them together in a single game tick.
    pub fn send_bundle<F>(&self, f: F)
    where
        F: FnOnce(&mut networking::BundleBuilder),
    {
        let mut builder = networking::BundleBuilder::new(self.connection.compression());
        f(&mut builder);
        let packets = builder.into_packets();
        if !packets.is_empty() {
            self.connection.send_encoded_bundle(packets);
        }
    }

    /// Disconnects the player with a reason message.
    pub fn disconnect(&self, reason: impl Into<TextComponent>) {
        self.connection.disconnect_with_reason(reason.into());
    }

    /// Ticks the player.
    #[allow(clippy::cast_possible_truncation)]
    pub fn tick(&self) {
        // Increment local tick counter
        self.tick_count.fetch_add(1, Ordering::Relaxed);

        // Reset first_good_position to current position at start of tick (vanilla: resetPosition)
        {
            let mut mv = self.movement.lock();
            mv.first_good_position = *self.position.lock();
            // Sync packet counts for rate limiting (vanilla: knownMovePacketCount = receivedMovePacketCount)
            mv.known_move_packet_count = mv.received_move_packet_count;
        }

        // Apply gravity to delta_movement (vanilla: applyGravity in Entity.tick/LivingEntity.travel)
        // This must happen after resetPosition so the speed check has the correct expected velocity
        self.apply_gravity();

        // Send pending block change acks (batched, once per tick like vanilla)
        self.tick_ack_block_changes();

        if !self.client_loaded.load(Ordering::Relaxed) {
            //return;
        }

        let current_pos = *self.position.lock();
        let chunk_x = (current_pos.x as i32) >> 4;
        let chunk_z = (current_pos.z as i32) >> 4;
        let chunk_pos = ChunkPos::new(chunk_x, chunk_z);

        *self.last_chunk_pos.lock() = chunk_pos;

        self.world.chunk_map.update_player_status(self);

        self.chunk_sender
            .lock()
            .send_next_chunks(self.connection.clone(), &self.world, chunk_pos);

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
            self.block_breaking.lock().tick(self, &self.world);
            self.check_below_world();

            // TODO: Implement remaining player ticking logic here
            // - Handling food/health regeneration
            // - Managing game mode specific logic
            // - Updating advancements
            // - Handling falling
        }

        // --- Post-tick (always runs, vanilla does not gate these behind isAlive) ---
        self.broadcast_inventory_changes();
        self.update_pose();
        self.sync_entity_data();

        // Only send CSetHealth when a value actually changed, matching vanilla's
        // `lastSentHealth` / `lastSentFood` / `lastFoodSaturationZero` pattern.
        {
            let health = *self.entity_data.lock().health.get();
            let food: i32 = 20; // TODO: use actual food level once hunger is implemented
            let saturation: f32 = 5.0; // TODO: use actual saturation once hunger is implemented
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
            let chunk_pos = *self.last_chunk_pos.lock();
            self.world.broadcast_to_nearby(
                chunk_pos,
                CEntityEvent {
                    entity_id: self.id,
                    event: EntityStatus::Poof,
                },
                None,
            );

            self.world
                .broadcast_to_all(CRemoveEntities::single(self.id));
            self.set_removed(RemovalReason::Killed);
        }
    }

    /// Syncs dirty entity data to nearby players.
    fn sync_entity_data(&self) {
        if let Some(dirty_values) = self.entity_data.lock().pack_dirty() {
            let packet = CSetEntityData::new(self.id, dirty_values);
            let chunk_pos = *self.last_chunk_pos.lock();
            self.world.broadcast_to_nearby(chunk_pos, packet, None);
        }
    }

    /// Attempts to pick up nearby item entities.
    ///
    /// Mirrors vanilla's `Player.aiStep()` item pickup logic:
    /// - Calculates pickup area as bounding box inflated by (1.0, 0.5, 1.0)
    /// - Calls `playerTouch()` on each entity in range
    fn touch_nearby_items(&self) {
        // Spectators can't pick up items
        if self.game_mode.load() == GameType::Spectator {
            return;
        }

        // Calculate pickup area (vanilla: Player.aiStep lines 454-458)
        let pickup_area = self.bounding_box().inflate_xyz(1.0, 0.5, 1.0);

        // Get all entities in the pickup area
        let entities = self.world.get_entities_in_aabb(&pickup_area);

        // Get player Arc for try_pickup (needed because try_pickup takes &Arc<Player>)
        let Some(player_arc) = self.world.players.get_by_entity_id(self.id) else {
            return;
        };

        for entity in entities {
            // Skip self
            if entity.id() == self.id {
                continue;
            }

            // Skip removed entities
            if entity.is_removed() {
                continue;
            }

            // Try to pick up item entities
            if let Some(item_entity) = entity.as_item_entity() {
                item_entity.try_pickup(&player_arc);
            }

            // TODO: Handle other entity types (experience orbs, arrows)
        }
    }

    /// Handles a custom payload packet.
    pub fn handle_custom_payload(&self, packet: SCustomPayload) {
        log::info!("Hello from the other side! {packet:?}");
    }

    /// Handles the end of a client tick.
    pub const fn handle_client_tick_end(&self) {
        //log::info!("Hello from the other side!");
    }

    /// Gets the next `messages_received` counter and increments it
    pub fn get_and_increment_messages_received(&self) -> i32 {
        let mut chat = self.chat.lock();
        let val = chat.messages_received;
        chat.messages_received += 1;
        val
    }

    fn verify_chat_signature(
        &self,
        packet: &SChat,
    ) -> Result<(message_chain::SignedMessageLink, LastSeen), String> {
        const MESSAGE_EXPIRES_AFTER: Duration = Duration::from_mins(5);

        let mut chat = self.chat.lock();
        let session = chat.chat_session.clone().ok_or("No chat session")?;
        let signature = packet.signature.as_ref().ok_or("No signature present")?;

        if session
            .profile_public_key
            .data()
            .has_expired_with_grace(profile_key::EXPIRY_GRACE_PERIOD)
        {
            return Err("Profile key has expired".to_string());
        }

        let chain = chat.message_chain.as_mut().ok_or("No message chain")?;

        if chain.is_broken() {
            return Err("Message chain is broken".to_string());
        }

        let timestamp =
            UNIX_EPOCH + Duration::from_millis(packet.timestamp.try_into().unwrap_or(0));

        let now = SystemTime::now();
        let message_age = now
            .duration_since(timestamp)
            .unwrap_or(Duration::from_secs(0));

        if message_age > MESSAGE_EXPIRES_AFTER {
            return Err(format!(
                "Message expired (age: {}s, max: 300s)",
                message_age.as_secs()
            ));
        }

        let last_seen_signatures = chat
            .message_validator
            .apply_update(packet.acknowledged, packet.offset, packet.checksum)
            .map_err(|e| {
                log::error!("Message acknowledgment validation failed: {e}");
                e
            })?;

        let last_seen = LastSeen::new(last_seen_signatures);

        let body = message_chain::SignedMessageBody::new(
            packet.message.clone(),
            timestamp,
            packet.salt,
            last_seen,
        );

        let chain = chat.message_chain.as_mut().ok_or("No message chain")?;
        let link = chain
            .validate_and_advance(&body)
            .map_err(|e| format!("Chain validation failed: {e}"))?;

        let updater = message_chain::MessageSignatureUpdater::new(&link, &body);
        let validator = session.profile_public_key.create_signature_validator();

        let is_valid = SignatureValidator::validate(&validator, &updater, signature)
            .map_err(|e| format!("Signature validation error: {e}"))?;

        if is_valid {
            Ok((link, body.last_seen.clone()))
        } else {
            Err("Invalid signature".to_string())
        }
    }

    /// Handles a chat message from the player.
    #[allow(clippy::too_many_lines)]
    pub fn handle_chat(&self, packet: SChat, player: Arc<Player>) {
        let chat_message = packet.message.clone();

        let verification_result = if let Some(_signature) = &packet.signature {
            match self.verify_chat_signature(&packet) {
                Ok((link, last_seen)) => {
                    // Don't add to cache here - will be added during broadcast
                    // to avoid cache state mismatch with client
                    Some(Ok((link, last_seen)))
                }
                Err(err) => {
                    log::warn!(
                        "Player {} sent message with invalid signature: {err}",
                        self.gameprofile.name
                    );
                    Some(Err(err))
                }
            }
        } else {
            None
        };

        if STEEL_CONFIG.enforce_secure_chat {
            match &verification_result {
                Some(Ok(_)) => {}
                Some(Err(err)) => {
                    self.disconnect(format!("Chat message validation failed: {err}"));
                    return;
                }
                None => {
                    self.disconnect(
                        "Secure chat is enforced on this server, but your message was not signed",
                    );
                    return;
                }
            }
        }

        let signature = if matches!(verification_result, Some(Ok(_))) {
            packet.signature.map(|sig| Box::new(sig) as Box<[u8]>)
        } else {
            None
        };

        let sender_index = {
            let mut chat = player.chat.lock();
            let idx = chat.messages_sent;
            chat.messages_sent += 1;
            idx
        };

        let registry_id = *REGISTRY.chat_types.get_id(vanilla_chat_types::CHAT) as i32;

        let chat_packet = CPlayerChat::new(
            0,
            player.gameprofile.id,
            sender_index,
            signature.clone(),
            chat_message.clone(),
            packet.timestamp,
            packet.salt,
            Box::new([]),
            Some(TextComponent::plain(chat_message.clone())),
            FilterType::PassThrough,
            ChatTypeBound {
                registry_id,
                sender_name: TextComponent::plain(player.gameprofile.name.clone())
                    .insertion(player.gameprofile.name.clone())
                    .click_event(ClickEvent::suggest_command(format!(
                        "/tell {} ",
                        player.gameprofile.name
                    )))
                    .hover_event(HoverEvent::show_entity(
                        "minecraft:player",
                        self.uuid(),
                        Some(player.gameprofile.name.clone()),
                    )),
                target_name: None,
            },
        );

        if let Some(sig_box) = &signature {
            if sig_box.len() == 256 {
                let mut sig_array = [0u8; 256];
                sig_array.copy_from_slice(&sig_box[..]);

                let last_seen = if let Some(Ok((_, ref last_seen))) = verification_result {
                    last_seen.clone()
                } else {
                    LastSeen::default()
                };

                steel_utils::chat!(player.gameprofile.name.clone(), "{}", chat_message);
                if let Some(server) = self.server.upgrade() {
                    for world in server.worlds.values() {
                        world.broadcast_chat(
                            chat_packet.clone(),
                            Arc::clone(&player),
                            last_seen.clone(),
                            Some(sig_array),
                        );
                    }
                }
            } else if let Some(server) = self.server.upgrade() {
                for world in server.worlds.values() {
                    world.broadcast_unsigned_chat(
                        chat_packet.clone(),
                        &player.gameprofile.name,
                        &chat_message,
                    );
                }
            }
        } else if let Some(server) = self.server.upgrade() {
            for world in server.worlds.values() {
                world.broadcast_unsigned_chat(
                    chat_packet.clone(),
                    &player.gameprofile.name,
                    &chat_message,
                );
            }
        }
    }

    /// Sends a system message to the player.
    pub fn send_message(&self, text: &TextComponent) {
        self.send_packet(CSystemChatMessage::new(text, self, false));
    }

    const fn is_invalid_position(x: f64, y: f64, z: f64, rot_x: f32, rot_y: f32) -> bool {
        if x.is_nan() || y.is_nan() || z.is_nan() {
            return true;
        }

        if !rot_x.is_finite() || !rot_y.is_finite() {
            return true;
        }

        false
    }

    /// Checks if we're awaiting a teleport confirmation and handles timeout/resend.
    ///
    /// Returns `true` if awaiting teleport (movement should be rejected),
    /// `false` if normal movement processing should continue.
    fn update_awaiting_teleport(&self) -> bool {
        let mut tp = self.teleport_state.lock();
        let Some(pos) = tp.awaiting_position else {
            tp.teleport_time = self.tick_count.load(Ordering::Relaxed);
            return false;
        };

        let current_tick = self.tick_count.load(Ordering::Relaxed);

        // Resend teleport after 20 ticks (~1 second) timeout
        if current_tick.wrapping_sub(tp.teleport_time) > 20 {
            tp.teleport_time = current_tick;
            let teleport_id = tp.teleport_id;
            drop(tp);

            let (yaw, pitch) = self.rotation.load();
            self.send_packet(CPlayerPosition::absolute(
                teleport_id,
                pos.x,
                pos.y,
                pos.z,
                yaw,
                pitch,
            ));
        }
        true // Still awaiting, reject movement
    }

    /// Marks that an impulse (knockback, etc.) was applied to the player.
    pub fn apply_impulse(&self) {
        self.movement.lock().last_impulse_tick = self.tick_count.load(Ordering::Relaxed);
    }

    /// Checks if movement validation should be performed for this player.
    ///
    /// Matches vanilla's `ServerGamePacketListenerImpl.shouldValidateMovement()`.
    /// Uses the `playerMovementCheck` and `elytraMovementCheck` gamerules.
    ///
    /// Returns `true` if movement should be validated, `false` to skip validation.
    fn should_validate_movement(&self, is_fall_flying: bool) -> bool {
        // Check playerMovementCheck gamerule
        let player_check = self.world.get_game_rule(PLAYER_MOVEMENT_CHECK);
        if player_check != GameRuleValue::Bool(true) {
            return false;
        }

        // If fall flying, also check elytraMovementCheck gamerule
        if is_fall_flying {
            let elytra_check = self.world.get_game_rule(ELYTRA_MOVEMENT_CHECK);
            return elytra_check == GameRuleValue::Bool(true);
        }

        true
    }

    /// Handles a move player packet.
    ///
    /// Matches vanilla `ServerGamePacketListenerImpl.handleMovePlayer()`.
    #[allow(clippy::cast_lossless, clippy::too_many_lines, clippy::similar_names)]
    pub fn handle_move_player(&self, packet: SMovePlayer) {
        if Self::is_invalid_position(
            packet.get_x(0.0),
            packet.get_y(0.0),
            packet.get_z(0.0),
            packet.get_x_rot(0.0),
            packet.get_y_rot(0.0),
        ) {
            self.disconnect(translations::MULTIPLAYER_DISCONNECT_INVALID_PLAYER_MOVEMENT.msg());
            return;
        }

        // Check awaiting teleport - if so, only update rotation (vanilla: absSnapRotationTo)
        if self.update_awaiting_teleport() {
            // While awaiting teleport, still allow rotation updates
            if packet.has_rot {
                self.rotation.store((packet.y_rot, packet.x_rot));
            }
            return;
        }

        if !self.client_loaded.load(Ordering::Relaxed) {
            return;
        }

        let (prev_pos, prev_rot) = {
            let mv = self.movement.lock();
            (mv.prev_position, mv.prev_rotation)
        };
        let start_pos = *self.position.lock();
        let game_mode = self.game_mode.load();
        let is_spectator = game_mode == GameType::Spectator;
        let is_creative = game_mode == GameType::Creative;
        let (is_sleeping, is_fall_flying, was_on_ground, is_crouching) = {
            let es = self.entity_state.lock();
            (es.sleeping, es.fall_flying, es.on_ground, es.crouching)
        };
        // Skip movement checks when tick rate is frozen (vanilla: tickRateManager().runsNormally())
        let tick_frozen = !self.world.tick_runs_normally();

        // Handle position updates
        if packet.has_pos {
            // Clamp position to vanilla limits
            let target_pos = Vector3::new(
                movement::clamp_horizontal(packet.position.x),
                movement::clamp_vertical(packet.position.y),
                movement::clamp_horizontal(packet.position.z),
            );
            let (first_good, last_good) = {
                let mv = self.movement.lock();
                (mv.first_good_position, mv.last_good_position)
            };

            // Sleeping check - only allow small movements when sleeping
            if is_sleeping {
                let dx = target_pos.x - first_good.x;
                let dy = target_pos.y - first_good.y;
                let dz = target_pos.z - first_good.z;
                let moved_dist_sq = dx * dx + dy * dy + dz * dz;

                if moved_dist_sq > 1.0 {
                    let (yaw, pitch) = self.rotation.load();
                    self.teleport(start_pos.x, start_pos.y, start_pos.z, yaw, pitch);
                    return;
                }
            } else {
                // Increment received packet count and calculate delta for rate limiting
                let mut delta_packets = {
                    let mut mv = self.movement.lock();
                    mv.received_move_packet_count += 1;
                    mv.received_move_packet_count - mv.known_move_packet_count
                };

                // Cap delta packets to prevent abuse (vanilla caps at 5)
                if delta_packets > 5 {
                    delta_packets = 1;
                }

                // Skip checks for spectators, creative mode, tick frozen, or gamerules disabled
                // Vanilla: shouldValidateMovement() checks playerMovementCheck and elytraMovementCheck
                let gamerule_skip = !self.should_validate_movement(is_fall_flying);
                let skip_checks = is_spectator || is_creative || tick_frozen || gamerule_skip;

                // Read movement state before building the input struct to avoid
                // holding the lock across the struct literal expression.
                let (expected_velocity_sq, in_impulse_grace) = {
                    let mv = self.movement.lock();
                    let vel_sq = mv.delta_movement_length_sq();
                    let current_tick = self.tick_count.load(Ordering::Relaxed);
                    let grace = current_tick.wrapping_sub(mv.last_impulse_tick)
                        < movement::IMPULSE_GRACE_TICKS;
                    (vel_sq, grace)
                };

                // Validate movement using physics simulation
                let mut validation = movement::validate_movement(
                    &self.world,
                    &movement::MovementInput {
                        target_pos,
                        first_good_pos: first_good,
                        last_good_pos: last_good,
                        expected_velocity_sq,
                        delta_packets,
                        is_fall_flying,
                        skip_checks,
                        in_impulse_grace,
                        is_crouching,
                        on_ground: was_on_ground,
                    },
                );

                if !validation.is_valid {
                    // Teleport back to start position
                    let (yaw, pitch) = prev_rot;
                    self.teleport(start_pos.x, start_pos.y, start_pos.z, yaw, pitch);
                    return;
                }

                // Movement accepted - update last good position
                self.movement.lock().last_good_position = target_pos;

                // Zero Y velocity when landing (vanilla: Block.updateEntityMovementAfterFallOn)
                // This prevents gravity from accumulating while on the ground
                if !was_on_ground && packet.on_ground {
                    validation.move_delta.y = 0.0;
                }
                // Update velocity based on actual movement (vanilla: handlePlayerKnownMovement)
                self.set_delta_movement(validation.move_delta);

                // Jump detection (vanilla: jumpFromGround)
                let moved_upwards = validation.move_delta.y > 0.0;
                if was_on_ground && !packet.on_ground && moved_upwards {
                    // Player jumped - could trigger jump-related mechanics here
                    // For now, this is a placeholder for future jump handling
                }
            }
        }

        // Update on_ground state from packet
        self.entity_state.lock().on_ground = packet.on_ground;

        // Update current state
        if packet.has_pos {
            let old_pos = *self.position.lock();
            *self.position.lock() = packet.position;

            // Notify callback of position change (updates entity cache section index)
            self.level_callback.lock().on_move(old_pos, packet.position);
        }
        if packet.has_rot {
            self.rotation.store((packet.y_rot, packet.x_rot));
        }

        // Broadcast movement to other players
        let pos = if packet.has_pos {
            packet.position
        } else {
            prev_pos
        };
        let (yaw, pitch) = if packet.has_rot {
            (packet.y_rot, packet.x_rot)
        } else {
            prev_rot
        };

        if packet.has_pos || packet.has_rot {
            let new_chunk = ChunkPos::new((pos.x as i32) >> 4, (pos.z as i32) >> 4);

            // Note: player_area_map is updated in chunk_map.update_player_status
            // which is called every tick and computes view diffs efficiently

            if packet.has_pos {
                let dx = calc_delta(pos.x, prev_pos.x);
                let dy = calc_delta(pos.y, prev_pos.y);
                let dz = calc_delta(pos.z, prev_pos.z);

                // Vanilla sync conditions (ServerEntity.java:148)
                let (sync_delay, last_on_ground) = {
                    let mut mv = self.movement.lock();
                    let d = mv.position_sync_delay;
                    mv.position_sync_delay += 1;
                    (d, mv.last_sent_on_ground)
                };
                let on_ground_changed = last_on_ground != packet.on_ground;
                let force_sync = sync_delay > 400 || on_ground_changed;

                if let (Some(dx), Some(dy), Some(dz)) = (dx, dy, dz) {
                    if force_sync {
                        // Send absolute position sync (forced by timer or on_ground change)
                        {
                            let mut mv = self.movement.lock();
                            mv.position_sync_delay = 0;
                            mv.last_sent_on_ground = packet.on_ground;
                        }

                        let delta = self.get_delta_movement();
                        let sync_packet = CEntityPositionSync {
                            entity_id: self.id,
                            x: pos.x,
                            y: pos.y,
                            z: pos.z,
                            velocity_x: delta.x,
                            velocity_y: delta.y,
                            velocity_z: delta.z,
                            yaw,
                            pitch,
                            on_ground: packet.on_ground,
                        };
                        self.world
                            .broadcast_to_nearby(new_chunk, sync_packet, Some(self.id));
                    } else {
                        let move_packet = CMoveEntityPosRot {
                            entity_id: self.id,
                            dx,
                            dy,
                            dz,
                            y_rot: to_angle_byte(yaw),
                            x_rot: to_angle_byte(pitch),
                            on_ground: packet.on_ground,
                        };
                        self.world
                            .broadcast_to_nearby(new_chunk, move_packet, Some(self.id));
                    }
                } else {
                    // Send absolute position sync (delta too big)
                    {
                        let mut mv = self.movement.lock();
                        mv.position_sync_delay = 0;
                        mv.last_sent_on_ground = packet.on_ground;
                    }

                    let delta = self.get_delta_movement();
                    let sync_packet = CEntityPositionSync {
                        entity_id: self.id,
                        x: pos.x,
                        y: pos.y,
                        z: pos.z,
                        velocity_x: delta.x,
                        velocity_y: delta.y,
                        velocity_z: delta.z,
                        yaw,
                        pitch,
                        on_ground: packet.on_ground,
                    };
                    self.world
                        .broadcast_to_nearby(new_chunk, sync_packet, Some(self.id));
                }
            } else {
                let rot_packet = CMoveEntityRot {
                    entity_id: self.id,
                    y_rot: to_angle_byte(yaw),
                    x_rot: to_angle_byte(pitch),
                    on_ground: packet.on_ground,
                };
                self.world
                    .broadcast_to_nearby(new_chunk, rot_packet, Some(self.id));
            }

            if packet.has_rot {
                let head_packet = CRotateHead {
                    entity_id: self.id,
                    head_y_rot: to_angle_byte(yaw),
                };
                self.world
                    .broadcast_to_nearby(new_chunk, head_packet, Some(self.id));
            }

            let mut mv = self.movement.lock();
            mv.prev_position = pos;
            mv.prev_rotation = (yaw, pitch);
        }
    }

    /// Updates the player's chat session and initializes the message chain.
    ///
    /// This should be called when receiving a `ChatSessionUpdate` packet from the client.
    pub fn set_chat_session(&self, session: RemoteChatSession) {
        // Initialize the message chain for this session
        let chain = SignedMessageChain::new(self.gameprofile.id, session.session_id);

        // Convert session to data for broadcasting
        let session_data = session.as_data();
        let protocol_data = match session_data.to_protocol_data() {
            Ok(data) => data,
            Err(err) => {
                log::error!(
                    "Failed to convert chat session to protocol data for {}: {:?}",
                    self.gameprofile.name,
                    err
                );
                let mut chat = self.chat.lock();
                chat.chat_session = Some(session);
                chat.message_chain = Some(chain);
                return;
            }
        };

        {
            let mut chat = self.chat.lock();
            chat.chat_session = Some(session);
            chat.message_chain = Some(chain);
        }

        log::info!(
            "Player {} initialized signed chat session",
            self.gameprofile.name
        );

        // Broadcast the chat session to all players so they can verify this player's signatures
        let update_packet =
            CPlayerInfoUpdate::update_chat_session(self.gameprofile.id, protocol_data);
        self.world.broadcast_to_all(update_packet);
    }

    /// Gets a reference to the player's chat session if present
    pub fn chat_session(&self) -> Option<RemoteChatSession> {
        self.chat.lock().chat_session.clone()
    }

    /// Checks if the player has a valid chat session
    pub fn has_chat_session(&self) -> bool {
        self.chat.lock().chat_session.is_some()
    }

    /// Handles a chat session update packet from the client.
    ///
    /// This validates the player's profile key and initializes signed chat if valid.
    pub fn handle_chat_session_update(&self, packet: SChatSessionUpdate) {
        log::info!("Player {} sent chat session update", self.gameprofile.name);

        // Convert the packet data to profile key data
        let expires_at = UNIX_EPOCH + Duration::from_millis(packet.expires_at as u64);

        // Decode the public key
        let public_key = match public_key_from_bytes(&packet.public_key) {
            Ok(key) => key,
            Err(err) => {
                log::warn!(
                    "Player {} sent invalid public key: {err}",
                    self.gameprofile.name
                );
                // Phase 4: Kick if enforcement is enabled
                if STEEL_CONFIG.enforce_secure_chat {
                    log::error!(
                        "Player {} kicked for invalid public key",
                        self.gameprofile.name
                    );
                    self.disconnect("Invalid profile public key");
                }
                return;
            }
        };

        let profile_key_data =
            profile_key::ProfilePublicKeyData::new(expires_at, public_key, packet.key_signature);

        let validator = Box::new(NoValidation) as Box<dyn SignatureValidator>;

        let session_data = profile_key::RemoteChatSessionData {
            session_id: packet.session_id,
            profile_public_key: profile_key_data,
        };

        match session_data.validate(self.gameprofile.id, &*validator) {
            Ok(session) => {
                self.set_chat_session(session);
            }
            Err(err) => {
                log::warn!(
                    "Player {} sent invalid chat session: {err}",
                    self.gameprofile.name
                );
                if STEEL_CONFIG.enforce_secure_chat {
                    self.disconnect(format!("Chat session validation failed: {err}"));
                }
            }
        }
    }

    /// Handles a chat acknowledgment packet from the client.
    pub fn handle_chat_ack(&self, packet: SChatAck) {
        if let Err(err) = self
            .chat
            .lock()
            .message_validator
            .apply_offset(packet.offset.0)
        {
            log::warn!(
                "Player {} sent invalid chat acknowledgment: {err}",
                self.gameprofile.name
            );
        }
    }

    /// Handles client information updates during play phase.
    pub fn handle_client_information(&self, packet: SClientInformation) {
        let old_view_distance = self.view_distance();

        let info = ClientInformation {
            language: packet.language,
            view_distance: packet.view_distance.clamp(2, 32) as u8,
            chat_visibility: packet.chat_visibility,
            chat_colors: packet.chat_colors,
            model_customisation: packet.model_customisation,
            main_hand: packet.main_hand,
            text_filtering_enabled: packet.text_filtering_enabled,
            allows_listing: packet.allows_listing,
            particle_status: packet.particle_status,
        };
        self.set_client_information(info);

        let new_view_distance = self.view_distance();
        if old_view_distance != new_view_distance {
            self.send_packet(CSetChunkCacheRadius {
                radius: i32::from(new_view_distance),
            });
        }
    }

    /// Sets the player's game mode and notifies the client.
    ///
    /// Returns `true` if the game mode was changed, `false` if the player was already in the requested game mode.
    pub fn set_game_mode(&self, gamemode: GameType) -> bool {
        let current_gamemode = self.game_mode.load();
        if current_gamemode == gamemode {
            return false;
        }

        self.prev_game_mode.store(self.game_mode.load());
        self.game_mode.store(gamemode);

        // Update abilities based on new game mode (mirrors vanilla GameType.updatePlayerAbilities)
        self.abilities.lock().update_for_game_mode(gamemode);

        // Send abilities first (vanilla sends this before game event)
        self.send_abilities();

        self.send_packet(CGameEvent {
            event: GameEventType::ChangeGameMode,
            data: gamemode.into(),
        });

        // Broadcast game mode update to all players (including self)
        // This updates PlayerInfo on clients, which is used for isSpectator() checks
        let update_packet =
            CPlayerInfoUpdate::update_game_mode(self.gameprofile.id, gamemode as i32);
        self.world.broadcast_to_all(update_packet);

        self.send_message(
            &translations::COMMANDS_GAMEMODE_SUCCESS_SELF
                .message([get_gamemode_translation(gamemode)])
                .into(),
        );

        true
    }

    /// Sends the player abilities packet to the client.
    /// This tells the client about flight, invulnerability, speeds, etc.
    pub fn send_abilities(&self) {
        let packet = self.abilities.lock().to_packet();
        self.send_packet(packet);
    }

    /// If the player's health is at or below zero (e.g. they disconnected while dead),
    /// resets health to 20.0 so they don't enter a zombie state on rejoin.
    /// Returns `true` if health was reset.
    pub fn reset_health_if_dead(&self) -> bool {
        let mut entity_data = self.entity_data.lock();
        let health = *entity_data.health.get();
        if health <= 0.0 {
            entity_data.health.set(20.0);
            drop(entity_data);

            let mut living_base = self.living_base.lock();
            living_base.reset_death_state();
            true
        } else {
            false
        }
    }

    /// Invalidates the delta-tracking state so that the next `tick()` will send
    /// `CSetHealth` to the client (vanilla: `resetSentInfo`).
    pub fn reset_sent_info(&self) {
        self.health_sync.lock().invalidate();
    }

    /// Handles a container button click packet (e.g., enchanting table buttons).
    pub fn handle_container_button_click(&self, packet: SContainerButtonClick) {
        log::debug!(
            "Player {} clicked button {} in container {}",
            self.gameprofile.name,
            packet.button_id,
            packet.container_id
        );
        // TODO: Implement container button click handling
        // This is used for things like:
        // - Enchanting table level selection
        // - Stonecutter recipe selection
        // - Loom pattern selection
        // - Lectern page turning
    }

    /// Handles a container click packet (slot interaction).
    pub fn handle_container_click(&self, packet: SContainerClick) {
        // First check if we have an open external menu
        let mut open_menu_guard = self.open_menu.lock();

        if let Some(ref mut menu) = *open_menu_guard {
            // Check container ID matches the open menu
            if i32::from(menu.container_id()) != packet.container_id {
                return;
            }

            // Handle the click using the open menu
            self.process_container_click(menu.as_mut(), packet);
        } else {
            // No external menu open, use the inventory menu
            drop(open_menu_guard);
            let mut menu = self.inventory_menu.lock();

            // Check container ID matches
            if i32::from(menu.behavior().container_id) != packet.container_id {
                return;
            }

            self.process_container_click(&mut *menu, packet);
        }
    }

    /// Processes a container click on any menu implementing the Menu trait.
    ///
    /// This is the common implementation shared between inventory menu and
    /// external menus (crafting table, chest, etc.).
    fn process_container_click(&self, menu: &mut dyn Menu, packet: SContainerClick) {
        // Handle spectator mode - just resync
        if self.game_mode.load() == GameType::Spectator {
            menu.behavior_mut()
                .send_all_data_to_remote(&self.connection);
            return;
        }

        // Validate slot index
        if !menu.behavior().is_valid_slot_index(packet.slot_num) {
            log::debug!(
                "Player {} clicked invalid slot index: {}, available: {}",
                self.gameprofile.name,
                packet.slot_num,
                menu.behavior().slot_count()
            );
            return;
        }

        // Check if we need a full resync (state ID mismatch)
        let full_resync_needed = packet.state_id as u32 != menu.behavior().get_state_id();

        // Suppress remote updates during click handling
        menu.behavior_mut().suppress_remote_updates();

        // Handle the click using the Menu trait method
        let has_infinite_materials = self.game_mode.load() == GameType::Creative;
        menu.clicked(
            packet.slot_num,
            packet.button_num,
            packet.click_type,
            has_infinite_materials,
            self,
        );

        // Update remote slots from the client's perception
        for (slot, hash) in packet.changed_slots {
            menu.behavior_mut().set_remote_slot(slot as usize, hash);
        }

        // Update remote carried from the client's perception
        menu.behavior_mut().set_remote_carried(packet.carried_item);

        // Resume remote updates
        menu.behavior_mut().resume_remote_updates();

        // Broadcast changes or full state depending on whether we had a state mismatch
        if full_resync_needed {
            menu.behavior_mut().broadcast_full_state(&self.connection);
        } else {
            menu.behavior_mut().broadcast_changes(&self.connection);
        }
    }

    /// Handles a container close packet.
    ///
    /// Based on Java's `ServerGamePacketListenerImpl::handleContainerClose`.
    pub fn handle_container_close(&self, packet: SContainerClose) {
        log::debug!(
            "Player {} closed container {}",
            self.gameprofile.name,
            packet.container_id
        );

        // Check if the closed container matches the currently open menu
        let open_menu = self.open_menu.lock();
        if let Some(ref menu) = *open_menu
            && i32::from(menu.container_id()) == packet.container_id
        {
            drop(open_menu);
            // Close the external menu (returns items to inventory)
            self.do_close_container();
            return;
        }
        drop(open_menu);

        // For the player inventory menu (container_id 0), call removed() to:
        // - Return crafting grid items to inventory
        // - Clear the cursor item
        if packet.container_id == i32::from(InventoryMenu::CONTAINER_ID) {
            let mut menu = self.inventory_menu.lock();
            menu.removed(self);
        }
    }

    /// Handles a container slot state changed packet (e.g., crafter slot toggle).
    pub fn handle_container_slot_state_changed(&self, packet: SContainerSlotStateChanged) {
        log::debug!(
            "Player {} changed slot {} state to {} in container {}",
            self.gameprofile.name,
            packet.slot_id,
            packet.new_state,
            packet.container_id
        );
        // TODO: Implement slot state change handling
        // This is used for the crafter block to enable/disable slots
    }

    /// Handles a creative mode slot set packet.
    pub fn handle_set_creative_mode_slot(&self, packet: SSetCreativeModeSlot) {
        // Only allow in creative mode
        if self.game_mode.load() != GameType::Creative {
            return;
        }

        let drop = packet.slot_num < 0;
        let item_stack = packet.item_stack;

        // Validate slot range (1-45 for inventory menu)
        let valid_slot = packet.slot_num >= 1 && packet.slot_num <= 45;
        let valid_data = item_stack.is_empty() || item_stack.count <= item_stack.max_stack_size();

        if valid_slot && valid_data {
            let mut menu = self.inventory_menu.lock();
            let slot_index = packet.slot_num as usize;

            {
                let mut guard = menu.behavior().lock_all_containers();
                if let Some(slot) = menu.behavior().get_slot(slot_index) {
                    slot.set_item(&mut guard, item_stack.clone());
                }
            }
            menu.behavior_mut()
                .set_remote_slot_known(slot_index, &item_stack);
            menu.behavior_mut().broadcast_changes(&self.connection);
        } else if drop && valid_data {
            // TODO: Implement drop spam throttling
            // For now, just drop the item
            if !item_stack.is_empty() {
                // TODO: Actually drop the item into the world
                log::debug!(
                    "Player {} would drop {:?} in creative mode",
                    self.gameprofile.name,
                    item_stack
                );
            }
        }
    }

    /// Acknowledges block changes up to the given sequence number.
    ///
    /// The ack is batched and sent once per tick (in `tick_ack_block_changes`),
    /// matching vanilla behavior.
    pub fn ack_block_changes_up_to(&self, sequence: i32) {
        let current = self.ack_block_changes_up_to.load(Ordering::Relaxed);
        if sequence > current {
            self.ack_block_changes_up_to
                .store(sequence, Ordering::Relaxed);
        }
    }

    /// Sends pending block change ack if any. Called once per tick.
    fn tick_ack_block_changes(&self) {
        let sequence = self.ack_block_changes_up_to.swap(-1, Ordering::Relaxed);
        if sequence > -1 {
            self.send_packet(CBlockChangedAck { sequence });
        }
    }

    /// Returns true if player is within block interaction range.
    ///
    /// Uses eye position and AABB distance (nearest point on block surface),
    /// matching vanilla's `Player.isWithinBlockInteractionRange(pos, 1.0)`.
    #[must_use]
    pub fn is_within_block_interaction_range(&self, pos: &BlockPos) -> bool {
        let player_pos = *self.position.lock();
        let eye_y = player_pos.y + self.get_eye_height();

        // Block AABB is [x, y, z] to [x+1, y+1, z+1]
        let min_x = f64::from(pos.x());
        let min_y = f64::from(pos.y());
        let min_z = f64::from(pos.z());
        let max_x = min_x + 1.0;
        let max_y = min_y + 1.0;
        let max_z = min_z + 1.0;

        // Distance from eye to nearest point on block AABB (0 if inside on that axis)
        let dx = f64::max(f64::max(min_x - player_pos.x, player_pos.x - max_x), 0.0);
        let dy = f64::max(f64::max(min_y - eye_y, eye_y - max_y), 0.0);
        let dz = f64::max(f64::max(min_z - player_pos.z, player_pos.z - max_z), 0.0);
        let dist_sq = dx * dx + dy * dy + dz * dz;

        // Base range is 4.5 blocks + 1.0 buffer
        let max_range = 4.5 + 1.0;
        dist_sq < max_range * max_range
    }

    /// Returns true if player is sneaking (secondary use active).
    #[must_use]
    pub fn is_secondary_use_active(&self) -> bool {
        self.entity_state.lock().crouching
    }

    /// Returns true if player has infinite materials (Creative mode).
    #[must_use]
    pub fn has_infinite_materials(&self) -> bool {
        self.game_mode.load() == GameType::Creative
    }

    /// Returns true if the player is currently sleeping.
    #[must_use]
    pub fn is_sleeping(&self) -> bool {
        self.entity_state.lock().sleeping
    }

    /// Sets the player's sleeping state.
    pub fn set_sleeping(&self, sleeping: bool) {
        self.entity_state.lock().sleeping = sleeping;
    }

    /// Returns true if the player is currently fall flying (elytra).
    #[must_use]
    pub fn is_fall_flying(&self) -> bool {
        self.entity_state.lock().fall_flying
    }

    /// Sets the player's fall flying state.
    pub fn set_fall_flying(&self, fall_flying: bool) {
        self.entity_state.lock().fall_flying = fall_flying;
    }

    /// Returns true if the player is flying (creative/spectator flight).
    #[must_use]
    pub fn is_flying(&self) -> bool {
        self.abilities.lock().flying
    }

    /// Sets the player's flying state.
    pub fn set_flying(&self, flying: bool) {
        self.abilities.lock().flying = flying;
    }

    /// Returns the player's flying speed.
    #[must_use]
    pub fn get_flying_speed(&self) -> f32 {
        self.abilities.lock().flying_speed
    }

    /// Sets the player's flying speed.
    pub fn set_flying_speed(&self, speed: f32) {
        self.abilities.lock().flying_speed = speed;
    }

    /// Returns a copy of the player's abilities.
    #[must_use]
    pub fn get_abilities(&self) -> Abilities {
        self.abilities.lock().clone()
    }

    /// Handles the player abilities packet from the client.
    /// This is sent when the player starts or stops flying.
    pub fn handle_player_abilities(&self, packet: SPlayerAbilities) {
        let mut abilities = self.abilities.lock();

        if abilities.may_fly {
            abilities.flying = packet.is_flying();
        } else if packet.is_flying() {
            // Client tried to fly but isn't allowed - resync abilities
            drop(abilities);
            self.send_abilities();
        }
    }

    /// Returns true if the player is on the ground.
    #[must_use]
    pub fn is_on_ground(&self) -> bool {
        self.entity_state.lock().on_ground
    }

    /// Determines the desired pose based on current player state.
    /// Priority: `Sleeping` > `FallFlying` > `Sneaking` > `Standing`
    // TODO: Add Swimming pose (requires water detection)
    // TODO: Add SpinAttack pose (requires riptide trident)
    // TODO: Add pose collision checks (force crouch in low ceilings)
    fn get_desired_pose(&self) -> EntityPose {
        let es = self.entity_state.lock();
        if es.sleeping {
            EntityPose::Sleeping
        } else if es.fall_flying {
            EntityPose::FallFlying
        } else if es.crouching && !self.abilities.lock().flying {
            EntityPose::Sneaking
        } else {
            EntityPose::Standing
        }
    }

    /// Updates the player's pose in entity data based on current state.
    fn update_pose(&self) {
        let desired_pose = self.get_desired_pose();
        self.entity_data.lock().pose.set(desired_pose);
    }

    /// Returns the player's client information settings.
    #[must_use]
    pub fn client_information(&self) -> ClientInformation {
        self.client_information.lock().clone()
    }

    /// Updates the player's client information settings.
    pub fn set_client_information(&self, info: ClientInformation) {
        *self.client_information.lock() = info;
    }

    /// Returns the effective view distance for this player.
    ///
    /// This is the minimum of the client's requested view distance and
    /// the server's configured maximum view distance.
    #[must_use]
    pub fn view_distance(&self) -> u8 {
        let client_view_distance = self.client_information.lock().view_distance;
        client_view_distance.min(STEEL_CONFIG.view_distance)
    }

    /// Returns the player's current velocity.
    #[must_use]
    pub fn get_delta_movement(&self) -> Vector3<f64> {
        self.movement.lock().delta_movement
    }

    /// Sets the player's velocity.
    pub fn set_delta_movement(&self, velocity: Vector3<f64>) {
        self.movement.lock().delta_movement = velocity;
    }

    /// Returns the player's current gravity value.
    ///
    /// Matches vanilla `LivingEntity.getGravity()` which reads from `Attributes.GRAVITY`.
    /// Default is 0.08 blocks/tick².
    const fn get_gravity(&self) -> f64 {
        // TODO: Read from attribute system when implemented
        let _ = self; // Silence unused warning until attributes are implemented
        movement::DEFAULT_GRAVITY
    }

    /// Applies gravity to the player's velocity.
    ///
    /// Matches vanilla `Entity.applyGravity()` and `LivingEntity.travel()`.
    /// Gravity is not applied when:
    /// - Player is on the ground
    /// - Player is in spectator mode (no physics)
    /// - Player is in creative mode and flying
    /// - Player is fall flying (elytra - uses different physics)
    fn apply_gravity(&self) {
        let (on_ground, is_fall_flying) = {
            let es = self.entity_state.lock();
            (es.on_ground, es.fall_flying)
        };
        let game_mode = self.game_mode.load();
        let is_spectator = game_mode == GameType::Spectator;
        let is_creative_flying = game_mode == GameType::Creative; // TODO: check actual flying state

        // Skip gravity when on ground, spectating, creative flying, or elytra flying
        if on_ground || is_spectator || is_creative_flying || is_fall_flying {
            return;
        }

        let gravity = self.get_gravity();
        if gravity != 0.0 {
            self.movement.lock().delta_movement.y -= gravity;
        }
    }

    /// Returns true if we're waiting for a teleport confirmation.
    #[must_use]
    pub fn is_awaiting_teleport(&self) -> bool {
        self.teleport_state.lock().is_awaiting()
    }

    /// Teleports the player to a new position.
    ///
    /// Sends a `CPlayerPosition` packet and waits for client acknowledgment.
    /// Until acknowledged, movement packets from the client will be rejected.
    ///
    /// Matches vanilla `ServerGamePacketListenerImpl.teleport()`.
    pub fn teleport(&self, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) {
        let pos = Vector3::new(x, y, z);

        let new_id = {
            let mut tp = self.teleport_state.lock();
            tp.teleport_time = self.tick_count.load(Ordering::Relaxed);
            let id = tp.next_id();
            tp.awaiting_position = Some(pos);
            id
        };

        // Update player position (vanilla: player.teleportSetPosition)
        *self.position.lock() = pos;
        self.rotation.store((yaw, pitch));

        // Send the teleport packet with the new ID
        self.send_packet(CPlayerPosition::absolute(new_id, x, y, z, yaw, pitch));
    }

    /// Handles a teleport acknowledgment from the client.
    ///
    /// Matches vanilla `ServerGamePacketListenerImpl.handleAcceptTeleportPacket()`.
    pub fn handle_accept_teleportation(&self, packet: SAcceptTeleportation) {
        let mut tp = self.teleport_state.lock();

        if let Some(pos) = tp.try_accept(packet.teleport_id) {
            // Snap player to awaited position (vanilla: player.absSnapTo)
            *self.position.lock() = pos;
            self.movement.lock().last_good_position = pos;
        } else if packet.teleport_id == tp.teleport_id && tp.awaiting_position.is_none() {
            // Client sent confirmation without server sending teleport
            drop(tp);
            self.disconnect(translations::MULTIPLAYER_DISCONNECT_INVALID_PLAYER_MOVEMENT.msg());
        }
        // If ID doesn't match, silently ignore (could be old/delayed packet)
    }

    /// Sends block update packets for a position and its neighbor.
    fn send_block_updates(&self, pos: &BlockPos, direction: Direction) {
        let state = self.world.get_block_state(pos);
        self.send_packet(CBlockUpdate {
            pos: *pos,
            block_state: state,
        });

        let neighbor_pos = direction.relative(pos);
        let neighbor_state = self.world.get_block_state(&neighbor_pos);
        self.send_packet(CBlockUpdate {
            pos: neighbor_pos,
            block_state: neighbor_state,
        });
    }

    /// Triggers arm swing animation and broadcasts it to nearby players.
    pub fn swing(&self, hand: InteractionHand, update_self: bool) {
        let action = match hand {
            InteractionHand::MainHand => AnimateAction::SwingMainHand,
            InteractionHand::OffHand => AnimateAction::SwingOffHand,
        };
        let packet = CAnimate::new(self.id, action);

        let chunk = *self.last_chunk_pos.lock();
        let exclude = if update_self { None } else { Some(self.id) };
        self.world.broadcast_to_nearby(chunk, packet, exclude);
    }

    /// Handles a player input packet (movement keys, sneaking, sprinting).
    pub fn handle_player_input(&self, packet: SPlayerInput) {
        self.entity_state.lock().crouching = packet.shift();
        // Note: sprinting is handled via SPlayerCommand packet
    }

    /// Handles the use of an item on a block.
    ///
    /// Implements the logic from Java's `ServerGamePacketListenerImpl.handleUseItemOn()`.
    pub fn handle_use_item_on(&self, packet: SUseItemOn) {
        // 1. Check client loaded
        if !self.client_loaded.load(Ordering::Relaxed) {
            return;
        }

        // 2. Ack block changes
        self.ack_block_changes_up_to(packet.sequence);

        let pos = &packet.block_hit.block_pos;
        let direction = packet.block_hit.direction;

        // 3. Validate interaction range
        if !self.is_within_block_interaction_range(pos) {
            self.send_block_updates(pos, direction);
            return;
        }

        // 4. Validate hit location precision (must be within 1.0000001 of block center)
        let center_x = f64::from(pos.x()) + 0.5;
        let center_y = f64::from(pos.y()) + 0.5;
        let center_z = f64::from(pos.z()) + 0.5;
        let location = &packet.block_hit.location;
        let limit = 1.000_000_1;

        if (location.x - center_x).abs() >= limit
            || (location.y - center_y).abs() >= limit
            || (location.z - center_z).abs() >= limit
        {
            log::warn!(
                "Rejecting UseItemOnPacket from {}: location {:?} too far from block {:?}",
                self.gameprofile.name,
                location,
                pos
            );
            self.send_block_updates(pos, direction);
            return;
        }

        // 5. Validate Y height
        if pos.y() >= self.world.max_build_height() {
            // TODO: Send "build.tooHigh" message to player
            self.send_block_updates(pos, direction);
            return;
        }

        // 6. Check awaiting teleport
        if self.is_awaiting_teleport() {
            self.send_block_updates(pos, direction);
            return;
        }

        // 7. Check may_interact permission
        if !self.world.may_interact(self, pos) {
            self.send_block_updates(pos, direction);
            return;
        }

        // 8. Call use_item_on
        let result = game_mode::use_item_on(self, &self.world, packet.hand, &packet.block_hit);

        // 9. Handle result
        if let InteractionResult::Success = result {
            // TODO: Trigger arm swing animation if needed
            self.swing(packet.hand, true);
        }

        // 10. Always send block updates to resync client
        self.send_block_updates(pos, direction);

        // 11. Broadcast inventory changes
        self.broadcast_inventory_changes();
    }

    /// Handles a player action packet (block breaking, item dropping, etc.).
    pub fn handle_player_action(&self, packet: SPlayerAction) {
        use block_breaking::BlockBreakAction;

        match packet.action {
            PlayerAction::StartDestroyBlock => {
                self.block_breaking.lock().handle_block_break_action(
                    self,
                    &self.world,
                    packet.pos,
                    BlockBreakAction::Start,
                    packet.direction,
                );
                // Ack after handler returns, matching vanilla
                self.ack_block_changes_up_to(packet.sequence);
            }
            PlayerAction::StopDestroyBlock => {
                self.block_breaking.lock().handle_block_break_action(
                    self,
                    &self.world,
                    packet.pos,
                    BlockBreakAction::Stop,
                    packet.direction,
                );
                // Ack after handler returns, matching vanilla
                self.ack_block_changes_up_to(packet.sequence);
            }
            PlayerAction::AbortDestroyBlock => {
                self.block_breaking.lock().handle_block_break_action(
                    self,
                    &self.world,
                    packet.pos,
                    BlockBreakAction::Abort,
                    packet.direction,
                );
                // Ack after handler returns, matching vanilla
                self.ack_block_changes_up_to(packet.sequence);
            }
            PlayerAction::DropAllItems => {
                self.drop_from_selected(true);
            }
            PlayerAction::DropItem => {
                self.drop_from_selected(false);
            }
            PlayerAction::ReleaseUseItem => {
                // TODO: Implement release use item (releasing bow, etc.)
                log::debug!("Player {} released use item", self.gameprofile.name);
            }
            PlayerAction::SwapItemWithOffhand => {
                // TODO: Implement swap item with offhand (F key)
                log::debug!("Player {} wants to swap items", self.gameprofile.name);
            }
            PlayerAction::Stab => {
                // Stab action for new combat system
                log::debug!("Player {} performed stab action", self.gameprofile.name);
            }
        }
    }

    /// Handles the use of an item.
    pub fn handle_use_item(&self, packet: SUseItem) {
        log::info!(
            "Player {} used {:?} (sequence: {}, yaw: {}, pitch: {})",
            self.gameprofile.name,
            packet.hand,
            packet.sequence,
            packet.y_rot,
            packet.x_rot
        );
        // TODO: Implement use item handler
    }

    /// Handles the pick block action (middle click on a block).
    ///
    /// # Panics
    ///
    /// Panics if the behavior registry has not been initialized.
    pub fn handle_pick_item_from_block(&self, packet: SPickItemFromBlock) {
        // Check if player is within interaction range (with 1.0 buffer like vanilla)
        if !self.is_within_block_interaction_range(&packet.pos) {
            return;
        }

        // Get block state at position
        let state = self.world.get_block_state(&packet.pos);
        if state.is_air() {
            return;
        }

        // Get the block and its behavior
        let block = state.get_block();
        let block_behaviors = &*BLOCK_BEHAVIORS;
        let behavior = block_behaviors.get_behavior(block);

        // Only include data if player has infinite materials (creative mode)
        let include_data = self.has_infinite_materials() && packet.include_data;

        // Get clone item stack from behavior (handles blocks with different item keys)
        let Some(item_stack) = behavior.get_clone_item_stack(block, state, include_data) else {
            // No corresponding item for this block (e.g., fire, portal)
            return;
        };

        if item_stack.is_empty() {
            return;
        }

        // TODO: If include_data, add block entity NBT data to the item stack
        // This requires block entity support which isn't implemented yet

        let mut inventory = self.inventory.lock();

        // Find existing slot with this item
        let slot_with_item = inventory.find_slot_matching_item(&item_stack);

        if slot_with_item != -1 {
            // Item found in inventory
            if PlayerInventory::is_hotbar_slot(slot_with_item as usize) {
                // Already in hotbar, just switch to that slot
                inventory.set_selected_slot(slot_with_item as u8);
            } else {
                // In main inventory, swap with current hotbar slot
                inventory.pick_slot(slot_with_item);
            }
        } else if self.has_infinite_materials() {
            // Creative mode: add item to inventory
            inventory.add_and_pick_item(item_stack);
        } else {
            // Survival mode and item not in inventory - do nothing
            return;
        }

        // Send updated held slot to client
        self.send_packet(CSetHeldSlot {
            slot: i32::from(inventory.get_selected_slot()),
        });

        // Broadcast inventory changes
        drop(inventory);
        self.inventory_menu
            .lock()
            .behavior_mut()
            .broadcast_changes(&self.connection);
    }

    /// Sets selected slot
    pub fn handle_set_carried_item(&self, packet: SSetCarriedItem) {
        self.inventory.lock().set_selected_slot(packet.slot as u8);
    }

    /// Handles a sign update packet from the client.
    pub fn handle_sign_update(&self, packet: SSignUpdate) {
        // Check if player is within interaction range
        if !self.is_within_block_interaction_range(&packet.pos) {
            return;
        }

        // Get the block entity at the position
        let Some(block_entity) = self.world.get_block_entity(&packet.pos) else {
            return;
        };

        // Lock and downcast to SignBlockEntity
        let mut guard = block_entity.lock();
        let Some(sign) = guard.as_any_mut().downcast_mut::<SignBlockEntity>() else {
            return;
        };

        // Check if sign is waxed (cannot be edited)
        if sign.is_waxed {
            return;
        }

        // Check if this player is allowed to edit the sign
        // Vanilla: player.getUUID().equals(sign.getPlayerWhoMayEdit())
        if sign.get_player_who_may_edit() != Some(self.gameprofile.id) {
            log::warn!(
                "Player {} tried to edit sign they're not allowed to edit",
                self.gameprofile.name
            );
            return;
        }

        // Update the sign text
        let text = sign.get_text_mut(packet.is_front_text);
        for (i, line) in packet.lines.iter().enumerate() {
            if i < 4 {
                // Create a plain text component from the line
                // Strip formatting codes (like vanilla does with ChatFormatting.stripFormatting)
                let stripped = strip_formatting_codes(line);
                text.set_message(i, TextComponent::plain(stripped));
            }
        }

        // Clear the edit lock now that we're done editing
        sign.set_player_who_may_edit(None);

        // Mark as changed (for persistence)
        sign.set_changed();

        // Get the update tag for broadcasting
        let update_tag = sign.get_update_tag();
        let block_entity_type = sign.get_type();
        let pos = packet.pos;

        // Release the lock before broadcasting
        drop(guard);

        // Broadcast block entity update to nearby players
        if let Some(nbt) = update_tag {
            self.world
                .broadcast_block_entity_update(pos, block_entity_type, nbt);
        }
    }

    /// Opens the sign editor for the player.
    ///
    /// # Arguments
    /// * `pos` - Position of the sign block
    /// * `is_front_text` - Whether to edit front (true) or back (false) text
    pub fn open_sign_editor(&self, pos: BlockPos, is_front_text: bool) {
        // Set this player as the one who may edit the sign
        if let Some(block_entity) = self.world.get_block_entity(&pos) {
            let mut guard = block_entity.lock();
            if let Some(sign) = guard.as_any_mut().downcast_mut::<SignBlockEntity>() {
                sign.set_player_who_may_edit(Some(self.gameprofile.id));
            }
        }

        // Send the block update first to ensure client has latest state
        let state = self.world.get_block_state(&pos);
        self.send_packet(CBlockUpdate {
            pos,
            block_state: state,
        });

        // Then open the sign editor
        self.send_packet(COpenSignEditor { pos, is_front_text });
    }

    /// Sends all inventory slots to the client (full sync).
    /// This should be called when the player first joins.
    pub fn send_inventory_to_remote(&self) {
        self.inventory_menu
            .lock()
            .behavior_mut()
            .send_all_data_to_remote(&self.connection);
    }

    // ==================== Menu Management ====================

    /// Generates the next container ID (1-100, wrapping around).
    ///
    /// Based on Java's `ServerPlayer::nextContainerCounter`.
    fn next_container_counter(&self) -> u8 {
        let current = self.container_counter.load(Ordering::Relaxed);
        let next = (current % 100) + 1;
        self.container_counter.store(next, Ordering::Relaxed);
        next
    }

    /// Opens a menu for this player.
    ///
    /// Based on Java's `ServerPlayer::openMenu`.
    ///
    /// # Arguments
    /// * `provider` - The menu provider containing the title and factory
    pub fn open_menu(&self, provider: &impl MenuProvider) {
        // Close any currently open menu first
        self.do_close_container();

        // Generate a new container ID and create the menu
        let container_id = self.next_container_counter();
        let mut menu = provider.create(container_id);

        // Send the open screen packet to the client
        self.send_packet(COpenScreen {
            container_id: i32::from(menu.container_id()),
            menu_type: menu.menu_type(),
            title: provider.title(),
        });

        // Send all slot data to the client
        menu.behavior_mut()
            .send_all_data_to_remote(&self.connection);

        // Store the menu
        *self.open_menu.lock() = Some(menu);
    }

    /// Closes the currently open container and returns to the inventory menu.
    ///
    /// Based on Java's `ServerPlayer::closeContainer`.
    /// This sends a close packet to the client.
    pub fn close_container(&self) {
        let open_menu = self.open_menu.lock();
        if let Some(menu) = &*open_menu {
            self.send_packet(CContainerClose {
                container_id: i32::from(menu.container_id()),
            });
        }
        drop(open_menu);
        self.do_close_container();
    }

    /// Internal close container logic without sending a packet.
    ///
    /// Based on Java's `ServerPlayer::doCloseContainer`.
    /// Called when the client sends a close packet or when opening a new menu.
    pub fn do_close_container(&self) {
        let mut open_menu = self.open_menu.lock();
        if let Some(ref mut menu) = *open_menu {
            menu.removed(self);
            // Transfer remote slot state from the container menu to the inventory menu.
            // This ensures the inventory menu knows what the client thinks it has in
            // the shared slots (player inventory), preventing unnecessary resyncs.
            self.inventory_menu
                .lock()
                .behavior_mut()
                .transfer_state(menu.behavior());
        }
        *open_menu = None;
    }

    /// Returns true if the player has an external menu open (not the inventory).
    #[must_use]
    pub fn has_container_open(&self) -> bool {
        self.open_menu.lock().is_some()
    }

    /// Broadcasts inventory changes to the client (incremental sync).
    /// This is called every tick to sync only changed slots.
    pub fn broadcast_inventory_changes(&self) {
        // First, broadcast changes for any open external menu
        let mut open_menu = self.open_menu.lock();
        if let Some(ref mut menu) = *open_menu {
            menu.behavior_mut().broadcast_changes(&self.connection);
        } else {
            drop(open_menu);
            // Only broadcast inventory menu changes if no external menu is open
            self.inventory_menu
                .lock()
                .behavior_mut()
                .broadcast_changes(&self.connection);
        }
    }

    /// Drops an item from the player's selected hotbar slot.
    ///
    /// Based on Java's `ServerPlayer.drop(boolean all)`.
    ///
    /// - `all`: If true, drops the entire stack (Ctrl+Q). If false, drops one item (Q).
    pub fn drop_from_selected(&self, all: bool) {
        if !self.can_drop_items() {
            return;
        }

        let removed = {
            let mut inventory = self.inventory.lock();
            let selected = inventory.get_selected_item_mut();
            if selected.is_empty() {
                return;
            }
            if all {
                selected.split(selected.count())
            } else {
                selected.split(1)
            }
        };

        self.drop_item(removed, false, true);
    }

    /// Drops an item into the world.
    ///
    /// Based on Java's `LivingEntity.drop(ItemStack, boolean randomly, boolean thrownFromHand)`.
    ///
    /// - `throw_randomly`: If true, the item is thrown in a random direction.
    ///   If false, it's thrown in the direction the player is facing.
    /// - `thrown_from_hand`: If true, sets the thrower and uses a longer pickup delay.
    pub fn drop_item(&self, item: ItemStack, throw_randomly: bool, thrown_from_hand: bool) {
        use std::f32::consts::TAU;

        if item.is_empty() {
            return;
        }

        let pos = self.position();
        let (yaw, pitch) = self.rotation.load();

        // Spawn position: eye height - 0.3 (hand level)
        // Vanilla: double yHandPos = this.getEyeY() - 0.3F
        let spawn_y = self.get_eye_y() - 0.3;

        // Calculate velocity based on throw type
        let velocity = if throw_randomly {
            // Random direction throw (like death drops)
            let power = rand::random::<f32>() * 0.5;
            let angle = rand::random::<f32>() * TAU;
            Vector3::new(
                f64::from(-angle.sin() * power),
                0.2,
                f64::from(angle.cos() * power),
            )
        } else {
            // Directional throw (player facing direction)
            let pitch_rad = pitch.to_radians();
            let yaw_rad = yaw.to_radians();

            let sin_pitch = pitch_rad.sin();
            let cos_pitch = pitch_rad.cos();
            let sin_yaw = yaw_rad.sin();
            let cos_yaw = yaw_rad.cos();

            // Random offset for slight variation
            let angle_offset = rand::random::<f32>() * TAU;
            let power_offset = 0.02 * rand::random::<f32>();

            Vector3::new(
                f64::from(-sin_yaw * cos_pitch * 0.3)
                    + f64::from(angle_offset.cos() * power_offset),
                f64::from(-sin_pitch * 0.3 + 0.1)
                    + f64::from((rand::random::<f32>() - rand::random::<f32>()) * 0.1),
                f64::from(cos_yaw * cos_pitch * 0.3) + f64::from(angle_offset.sin() * power_offset),
            )
        };

        let spawn_pos = Vector3::new(pos.x, spawn_y, pos.z);

        if let Some(entity) = self
            .world
            .spawn_item_with_velocity(spawn_pos, item, velocity)
        {
            // Set pickup delay: 40 ticks (2 seconds) when thrown from hand
            if thrown_from_hand {
                entity.set_pickup_delay(40);
                entity.set_thrower(self.gameprofile.id);
            }
        }
    }

    /// Returns true if the player can drop items.
    ///
    /// Based on Java's `Player.canDropItems()`.
    /// Returns false if the player is dead, removed, or has a flag preventing item drops.
    #[must_use]
    pub fn can_drop_items(&self) -> bool {
        // Check if player is removed
        !self.removed.load(Ordering::Relaxed)
        // TODO: Check if player is alive (health > 0)
    }

    /// Tries to add an item to the player's inventory, dropping it if it doesn't fit.
    ///
    /// Based on Java's `Inventory.placeItemBackInInventory`.
    pub fn add_item_or_drop(&self, mut item: ItemStack) {
        if item.is_empty() {
            return;
        }

        // Try to add to inventory
        let added = self.inventory.lock().add(&mut item);
        if !added || !item.is_empty() {
            // Couldn't fit everything, drop the rest
            self.drop_item(item, false, false);
        }
    }

    /// Tries to add an item to the player's inventory using an existing lock guard,
    /// dropping it if it doesn't fit.
    ///
    /// Use this variant when you already hold a `ContainerLockGuard` that includes
    /// the player's inventory to avoid deadlocks.
    pub fn add_item_or_drop_with_guard(&self, guard: &mut ContainerLockGuard, mut item: ItemStack) {
        if item.is_empty() {
            return;
        }

        let inv_id = ContainerId::from_arc(&self.inventory);
        if let Some(inv) = guard.get_mut(inv_id) {
            let added = inv.add(&mut item);
            if !added || !item.is_empty() {
                self.drop_item(item, false, false);
            }
        } else {
            // Inventory not in guard - this shouldn't happen but drop the item to be safe
            self.drop_item(item, false, false);
        }
    }

    fn check_below_world(&self) {
        let pos = *self.position.lock();
        if pos.y < f64::from(self.world.get_min_y() - 64) {
            self.hurt(
                &DamageSource::environment(vanilla_damage_types::OUT_OF_WORLD),
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
        {
            let abilities = self.abilities.lock();
            if abilities.invulnerable && !source.bypasses_invulnerability() {
                return false;
            }
        }

        if *self.entity_data.lock().health.get() <= 0.0 {
            return false;
        }

        // TODO: gamerule damage-type checks (drowningDamage, fallDamage, etc.)
        // TODO: difficulty scaling (Peaceful/Easy/Hard)
        if source.scales_with_difficulty() {
            // needs todo
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

        self.actually_hurt(source, effective_amount);

        if took_full_damage {
            let type_id = *REGISTRY.damage_types.get_id(source.damage_type) as i32;
            let chunk_pos = *self.last_chunk_pos.lock();

            self.world.broadcast_to_nearby(
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
            self.world.broadcast_to_nearby(
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
    /// Vanilla: `LivingEntity.actuallyHurt()`
    /// TODO: armor, enchantment, absorption, food exhaustion
    fn actually_hurt(&self, _source: &DamageSource, amount: f32) {
        // TODO: apply armor/enchant/absorption reductions here (vanilla: getDamageAfterArmorAbsorb, getDamageAfterMagicAbsorb)
        // TODO: absorption amount handling
        // TODO: food exhaustion (source.getFoodExhaustion())
        // TODO: combat tracker (getCombatTracker().recordDamage)
        if amount <= 0.0 {
            return;
        }

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

        // NOTE: Vanilla `ServerPlayer.die()` does NOT set Pose::Dying — only
        // `LivingEntity.die()` does (which ServerPlayer never calls via super).
        // The death screen covers the player model, so the pose is irrelevant.

        // Broadcast entity event 3 (death sound) to all nearby players.
        let chunk_pos = *self.last_chunk_pos.lock();
        self.world.broadcast_to_nearby(
            chunk_pos,
            CEntityEvent {
                entity_id: self.id,
                event: EntityStatus::Death,
            },
            None,
        );

        let show_death_messages =
            self.world.get_game_rule(SHOW_DEATH_MESSAGES) == GameRuleValue::Bool(true);

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
            self.world.broadcast_system_chat(CSystemChat {
                content: death_message,
                overlay: false,
            });
        }

        if self.world.get_game_rule(KEEP_INVENTORY) != GameRuleValue::Bool(true) {
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

        if self.world.get_game_rule(IMMEDIATE_RESPAWN) == GameRuleValue::Bool(true) {
            self.respawn();
        }
    }

    /// TODO: bed/respawn anchor, cross-dimension, potion clearing, noRespawnBlockAvailable
    ///
    /// # Panics
    /// If the player dies in a dimension that doesn't exist.
    #[allow(clippy::too_many_lines)]
    pub fn respawn(&self) {
        {
            let mut living_base = self.living_base.lock();
            if !living_base.dead {
                return;
            }
            living_base.reset_death_state();
        };

        let was_removed = self.removed.swap(false, Ordering::AcqRel);

        let world = &self.world;

        // Only send CRemoveEntities if tick_death() hasn't already removed us
        // (tick_death sends CRemoveEntities + set_removed at DEATH_DURATION).
        // NOTE: Since we reuse the same entity ID (unlike vanilla which creates a
        // fresh ServerPlayer), clients may briefly see remove+re-add in the same
        // frame if respawn races with tick_death's DEATH_DURATION removal.
        if !was_removed {
            world.broadcast_to_all(CRemoveEntities::single(self.id));
        }

        // Reset transient state. Vanilla creates a fresh ServerPlayer so all state
        // is naturally zeroed; we reuse the same Player, so we must reset manually.
        // TODO: as new transient fields are added (effects, fire ticks, frozen
        // ticks, etc.), they must be reset here too.
        self.movement.lock().delta_movement = Vector3::default();
        {
            let mut es = self.entity_state.lock();
            es.on_ground = false;
            es.fall_flying = false;
            es.sleeping = false;
            es.crouching = false;
        }
        *self.block_breaking.lock() = BlockBreakingManager::new();

        {
            let mut entity_data = self.entity_data.lock();
            entity_data.health.set(20.0);
            entity_data.pose.set(EntityPose::Standing);
        }

        self.health_sync.lock().reset_for_respawn();

        let dimension_key = world.dimension.key.clone();
        let dimension_type_id = *(REGISTRY.dimension_types.get_id(
            REGISTRY
                .dimension_types
                .by_key(&dimension_key)
                .expect("Dimension should be registered!"),
        )) as i32;

        // TODO: bed/respawn anchor lookup, send NO_RESPAWN_BLOCK_AVAILABLE if missing

        self.send_packet(CRespawn {
            dimension_type: dimension_type_id,
            dimension_name: dimension_key.clone(),
            hashed_seed: world.obfuscated_seed(),
            gamemode: self.game_mode.load() as u8,
            previous_gamemode: self.prev_game_mode.load() as i8,
            is_debug: false,
            is_flat: matches!(STEEL_CONFIG.world_generator, WorldGeneratorTypes::Flat),
            has_death_location: false,
            death_dimension_name: None,
            death_location: None,
            portal_cooldown_ticks: 0,
            // TODO: read from dimension's noise_settings (varies per dimension, e.g. nether=32, end=0)
            sea_level: 63,
            data_kept: 0,
        });

        let spawn_pos = world.level_data.read().data().spawn_pos();
        let spawn = Vector3::new(
            f64::from(spawn_pos.x()) + 0.5,
            f64::from(spawn_pos.y()),
            f64::from(spawn_pos.z()) + 0.5,
        );
        *self.position.lock() = spawn;
        {
            let mut mv = self.movement.lock();
            mv.prev_position = spawn;
            mv.last_good_position = spawn;
            mv.first_good_position = spawn;
        }
        self.rotation.store((0.0, 0.0));
        self.teleport(spawn.x, spawn.y, spawn.z, 0.0, 0.0);

        // TODO: send CSetDefaultSpawnPosition (dimension, pos, yaw, pitch)

        // TODO: send CChangeDifficulty (difficulty, locked)

        // TODO: send CSetExperience (progress, level, total)

        // TODO: send mob effect packets once effects are implemented

        // TODO: send CInitializeBorder once world border is implemented

        // Vanilla: ChunkMap.addEntity -> addPairing -> sendPairingData
        // TODO: also send SetEquipment + UpdateAttributes in the bundle
        let player_type_id = *REGISTRY.entity_types.get_id(vanilla_entities::PLAYER) as i32;
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

        // TODO: sendPlayerPermissionLevel
        // TODO: initInventoryMenu

        {
            let level_data = world.level_data.read();
            let game_time = level_data.game_time();
            let day_time = level_data.day_time();
            drop(level_data);

            let advance_time = world
                .get_game_rule(ADVANCE_TIME)
                .as_bool()
                .expect("gamerule advance_time should always be a bool.");
            self.send_packet(CSetTime {
                game_time,
                day_time,
                time_of_day_increasing: advance_time,
            });
        }

        if world.is_raining() {
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

        self.send_packet(CGameEvent {
            event: GameEventType::LevelChunksLoadStart,
            data: 0.0,
        });

        // TODO: tick rate update for joining player

        // --- 6) Re-enter chunk tracking (vanilla: addEntity -> updatePlayerStatus) ---
        world.player_area_map.remove_by_entity_id(self.id);
        world.chunk_map.remove_player(self);
        world.entity_tracker().on_player_leave(self.id);
        self.client_loaded.store(false, Ordering::Relaxed);

        self.send_abilities();
        self.send_inventory_to_remote();
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
    pub const fn cleanup(&self) {}
}

impl Entity for Player {
    fn entity_type(&self) -> EntityTypeRef {
        vanilla_entities::PLAYER
    }

    fn id(&self) -> i32 {
        self.id
    }

    fn uuid(&self) -> Uuid {
        self.gameprofile.id
    }

    fn position(&self) -> Vector3<f64> {
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
        // Player tick is handled separately by World::tick_b()
        // This is here for Entity trait compliance
    }

    fn level(&self) -> Option<Arc<World>> {
        Some(Arc::clone(&self.world))
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

    fn velocity(&self) -> Vector3<f64> {
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
}

impl LivingEntity for Player {
    fn get_health(&self) -> f32 {
        *self.entity_data.lock().health.get()
    }

    fn set_health(&self, health: f32) {
        let max_health = self.get_max_health();
        let clamped = health.clamp(0.0, max_health);
        self.entity_data.lock().health.set(clamped);
    }

    fn get_max_health(&self) -> f32 {
        // TODO: Get from attributes system when implemented
        20.0
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
        // Dirty flag set automatically, will sync on next tick
    }

    fn get_armor_value(&self) -> i32 {
        // TODO: Calculate from equipped items when data components are implemented
        // Will iterate over ARMOR_SLOTS and sum armor values from each piece
        0
    }

    fn is_sprinting(&self) -> bool {
        self.entity_state.lock().sprinting
    }

    fn set_sprinting(&self, sprinting: bool) {
        self.entity_state.lock().sprinting = sprinting;
        // TODO: Apply speed modifiers when attribute system is implemented
    }

    fn get_speed(&self) -> f32 {
        self.speed.load()
    }

    fn set_speed(&self, speed: f32) {
        self.speed.store(speed);
    }
}

/// Strips Minecraft formatting codes (§ followed by a character) from a string.
///
/// This is equivalent to vanilla's `ChatFormatting.stripFormatting()`.
fn strip_formatting_codes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '§' {
            // Skip the formatting code character if present
            chars.next();
        } else {
            result.push(c);
        }
    }

    result
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
