//! This module contains all things player-related.
mod abilities;
pub mod block_breaking;
pub mod chunk_sender;
mod game_mode;
mod game_profile;
pub mod message_chain;
mod message_validator;
pub mod movement;
/// This module contains the networking implementation for the player.
pub mod networking;
pub mod player_data;
pub mod player_data_storage;
pub mod player_inventory;
pub mod profile_key;
mod signature_cache;

pub use abilities::Abilities;

use block_breaking::BlockBreakingManager;
use crossbeam::atomic::AtomicCell;
pub use game_profile::{GameProfile, GameProfileAction};
use message_chain::SignedMessageChain;
use message_validator::LastSeenMessagesValidator;
use profile_key::RemoteChatSession;
pub use signature_cache::{LastSeen, MessageCache};
use std::{
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use steel_protocol::packets::game::CSystemChatMessage;
use steel_protocol::packets::game::{
    AnimateAction, CAnimate, CEntityPositionSync, COpenSignEditor, CPlayerPosition, CSetEntityData,
    CSetHeldSlot, PlayerAction, SAcceptTeleportation, SPickItemFromBlock, SPlayerAbilities,
    SPlayerAction, SSetCarriedItem, SUseItem, SUseItemOn,
};
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::shapes::AABBd;
use steel_registry::entity_data::EntityPose;
use steel_registry::entity_types::EntityTypeRef;
use steel_registry::game_rules::GameRuleValue;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::PlayerEntityData;
use steel_registry::vanilla_game_rules::{ELYTRA_MOVEMENT_CHECK, PLAYER_MOVEMENT_CHECK};
use steel_registry::{REGISTRY, vanilla_chat_types};

use steel_utils::locks::SyncMutex;
use steel_utils::types::GameType;
use text_components::resolving::TextResolutor;
use text_components::{Modifier, TextComponent};
use text_components::{
    content::Resolvable,
    custom::CustomData,
    interactivity::{ClickEvent, HoverEvent},
};
use uuid::Uuid;

use crate::inventory::SyncPlayerInv;
use crate::player::player_inventory::PlayerInventory;
use crate::server::Server;
use crate::{
    config::STEEL_CONFIG,
    entity::{Entity, EntityLevelCallback, NullEntityCallback, RemovalReason},
};

use steel_crypto::{SignatureValidator, public_key_from_bytes, signature::NoValidation};
use steel_protocol::packets::{
    common::{SClientInformation, SCustomPayload},
    game::{
        CBlockChangedAck, CBlockUpdate, CContainerClose, CGameEvent, CMoveEntityPosRot,
        CMoveEntityRot, COpenScreen, CPlayerChat, CPlayerInfoUpdate, CRotateHead,
        CSetChunkCacheRadius, ChatTypeBound, FilterType, GameEventType, PreviousMessage, SChat,
        SChatAck, SChatSessionUpdate, SContainerButtonClick, SContainerClick, SContainerClose,
        SContainerSlotStateChanged, SMovePlayer, SPlayerInput, SSetCreativeModeSlot, SSignUpdate,
        calc_delta, to_angle_byte,
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

use crate::chunk::player_chunk_view::PlayerChunkView;
use crate::player::{chunk_sender::ChunkSender, networking::JavaConnection};
use crate::world::World;

/// A struct representing a player.
pub struct Player {
    /// The player's game profile.
    pub gameprofile: GameProfile,
    /// The player's connection.
    pub connection: Arc<JavaConnection>,

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
    /// The previous position for delta movement calculations.
    prev_position: SyncMutex<Vector3<f64>>,
    /// The previous rotation for movement broadcasts.
    prev_rotation: AtomicCell<(f32, f32)>,

    /// Synchronized entity data (health, pose, flags, etc.) for network sync.
    entity_data: SyncMutex<PlayerEntityData>,

    /// The player's movement speed.
    speed: AtomicCell<f32>,
    /// Whether the player is sprinting.
    sprinting: AtomicBool,

    /// The last chunk position of the player.
    pub last_chunk_pos: SyncMutex<ChunkPos>,
    /// The last chunk tracking view of the player.
    pub last_tracking_view: SyncMutex<Option<PlayerChunkView>>,
    /// The chunk sender for the player.
    pub chunk_sender: SyncMutex<ChunkSender>,

    /// The client's settings/information (language, view distance, chat visibility, etc.).
    /// Updated when the client sends `SClientInformation` during config or play phase.
    client_information: SyncMutex<ClientInformation>,

    /// Counter for chat messages sent BY this player
    messages_sent: AtomicI32,
    /// Counter for chat messages received BY this player
    messages_received: AtomicI32,

    /// Message signature cache for tracking chat messages
    pub signature_cache: SyncMutex<MessageCache>,

    /// Validator for client acknowledgements of messages we've sent
    pub message_validator: SyncMutex<LastSeenMessagesValidator>,

    /// Remote chat session containing the player's public key (if signed chat is enabled)
    pub chat_session: SyncMutex<Option<RemoteChatSession>>,

    /// Message chain state for tracking signed message sequence
    pub message_chain: SyncMutex<Option<SignedMessageChain>>,

    /// The player's current game mode (Survival, Creative, Adventure, Spectator)
    pub game_mode: AtomicCell<GameType>,

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

    /// Whether the player is sneaking (shift key down).
    shift_key_down: AtomicBool,

    /// Position we're waiting for the client to confirm via teleport ack.
    /// If Some, we should reject interaction packets until confirmed.
    awaiting_position_from_client: SyncMutex<Option<Vector3<f64>>>,

    /// Incrementing teleport ID counter (wraps at `i32::MAX`).
    awaiting_teleport_id: AtomicI32,

    /// Tick count when last teleport was sent (for timeout/resend).
    awaiting_teleport_time: AtomicI32,

    /// Local tick counter (incremented each tick).
    tick_count: AtomicI32,

    /// Last known good position (for collision rollback).
    last_good_position: SyncMutex<Vector3<f64>>,

    /// Position at start of tick (for speed validation).
    /// Matches vanilla `firstGoodX/Y/Z`.
    first_good_position: SyncMutex<Vector3<f64>>,

    /// Number of move packets received since connection started.
    received_move_packet_count: AtomicI32,

    /// Number of move packets at the last tick (for rate limiting).
    known_move_packet_count: AtomicI32,

    /// Player's current velocity (delta movement per tick).
    /// Used for speed validation in movement checks.
    delta_movement: SyncMutex<Vector3<f64>>,

    /// Whether the player is currently sleeping in a bed.
    sleeping: AtomicBool,

    /// Player abilities (flight, invulnerability, build permissions, speeds, etc.)
    abilities: SyncMutex<Abilities>,

    /// Whether the player is currently fall flying (elytra gliding).
    fall_flying: AtomicBool,

    /// Whether the player is on the ground.
    on_ground: AtomicBool,

    /// Tick when last impulse was applied (knockback, etc.).
    /// Used for post-impulse grace period during movement validation.
    last_impulse_tick: AtomicI32,

    /// Block breaking state machine.
    pub block_breaking: SyncMutex<BlockBreakingManager>,

    /// Tick counter for forced position sync (resets to 0 after sync, like vanilla teleportDelay).
    position_sync_delay: AtomicI32,

    /// Last `on_ground` state sent to tracking players (for detecting changes).
    last_sent_on_ground: AtomicBool,

    /// Whether the player has been removed from the world.
    removed: AtomicBool,

    /// Callback for entity lifecycle events (movement between chunks, removal).
    level_callback: SyncMutex<Arc<dyn EntityLevelCallback>>,
}

impl Player {
    /// Creates a new player.
    pub fn new(
        gameprofile: GameProfile,
        connection: Arc<JavaConnection>,
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
            prev_position: SyncMutex::new(pos),
            prev_rotation: AtomicCell::new((0.0, 0.0)),
            entity_data: SyncMutex::new(PlayerEntityData::new()),
            speed: AtomicCell::new(0.1), // Default walking speed
            sprinting: AtomicBool::new(false),
            last_chunk_pos: SyncMutex::new(ChunkPos::new(0, 0)),
            last_tracking_view: SyncMutex::new(None),
            chunk_sender: SyncMutex::new(ChunkSender::default()),
            client_information: SyncMutex::new(client_information),
            messages_sent: AtomicI32::new(0),
            messages_received: AtomicI32::new(0),
            signature_cache: SyncMutex::new(MessageCache::new()),
            message_validator: SyncMutex::new(LastSeenMessagesValidator::new()),
            chat_session: SyncMutex::new(None),
            message_chain: SyncMutex::new(None),
            game_mode: AtomicCell::new(GameType::Survival),
            inventory: inventory.clone(),
            inventory_menu: SyncMutex::new(InventoryMenu::new(inventory)),
            open_menu: SyncMutex::new(None),
            container_counter: AtomicU8::new(0),
            ack_block_changes_up_to: AtomicI32::new(-1),
            shift_key_down: AtomicBool::new(false),
            awaiting_position_from_client: SyncMutex::new(None),
            awaiting_teleport_id: AtomicI32::new(0),
            awaiting_teleport_time: AtomicI32::new(0),
            tick_count: AtomicI32::new(0),
            last_good_position: SyncMutex::new(Vector3::default()),
            first_good_position: SyncMutex::new(Vector3::default()),
            received_move_packet_count: AtomicI32::new(0),
            known_move_packet_count: AtomicI32::new(0),
            delta_movement: SyncMutex::new(Vector3::default()),
            sleeping: AtomicBool::new(false),
            abilities: SyncMutex::new(Abilities::default()),
            fall_flying: AtomicBool::new(false),
            on_ground: AtomicBool::new(false),
            last_impulse_tick: AtomicI32::new(0),
            block_breaking: SyncMutex::new(BlockBreakingManager::new()),
            position_sync_delay: AtomicI32::new(0),
            last_sent_on_ground: AtomicBool::new(false),
            removed: AtomicBool::new(false),
            level_callback: SyncMutex::new(Arc::new(NullEntityCallback)),
        }
    }

    /// Ticks the player.
    #[allow(clippy::cast_possible_truncation)]
    pub fn tick(&self) {
        // Increment local tick counter
        self.tick_count.fetch_add(1, Ordering::Relaxed);

        // Reset first_good_position to current position at start of tick (vanilla: resetPosition)
        *self.first_good_position.lock() = *self.position.lock();

        // Apply gravity to delta_movement (vanilla: applyGravity in Entity.tick/LivingEntity.travel)
        // This must happen after resetPosition so the speed check has the correct expected velocity
        self.apply_gravity();

        // Sync packet counts for rate limiting (vanilla: knownMovePacketCount = receivedMovePacketCount)
        self.known_move_packet_count.store(
            self.received_move_packet_count.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );

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

        // Try to pick up nearby items (vanilla: Player.aiStep)
        self.touch_nearby_items();

        // Broadcast inventory changes to client
        self.broadcast_inventory_changes();

        // Tick block breaking
        self.block_breaking.lock().tick(self, &self.world);

        // Update pose based on current state
        self.update_pose();

        // Sync dirty entity data to nearby players
        self.sync_entity_data();

        self.connection.tick();

        // TODO: Implement player ticking logic here
        // This will include:
        // - Checking if the player is alive
        // - Handling movement
        // - Updating inventory
        // - Handling food/health regeneration
        // - Managing game mode specific logic
        // - Updating advancements
        // - Handling falling
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
        self.messages_received.fetch_add(1, Ordering::Relaxed)
    }

    fn verify_chat_signature(
        &self,
        packet: &SChat,
    ) -> Result<(message_chain::SignedMessageLink, LastSeen), String> {
        const MESSAGE_EXPIRES_AFTER: Duration = Duration::from_mins(5);

        let session = self.chat_session.lock().clone().ok_or("No chat session")?;
        let signature = packet.signature.as_ref().ok_or("No signature present")?;

        if session
            .profile_public_key
            .data()
            .has_expired_with_grace(profile_key::EXPIRY_GRACE_PERIOD)
        {
            return Err("Profile key has expired".to_string());
        }

        let mut chain_guard = self.message_chain.lock();
        let chain = chain_guard.as_mut().ok_or("No message chain")?;

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

        let last_seen_signatures = self
            .message_validator
            .lock()
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
                    self.connection
                        .disconnect(format!("Chat message validation failed: {err}"));
                    return;
                }
                None => {
                    self.connection.disconnect(
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

        let sender_index = player.messages_sent.fetch_add(1, Ordering::SeqCst);

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

                log::info!("<{}> {}", player.gameprofile.name, chat_message);
                self.world.broadcast_chat(
                    chat_packet,
                    Arc::clone(&player),
                    last_seen,
                    Some(sig_array),
                );
            } else {
                self.world.broadcast_unsigned_chat(
                    chat_packet,
                    &player.gameprofile.name,
                    &chat_message,
                );
            }
        } else {
            self.world.broadcast_unsigned_chat(
                chat_packet,
                &player.gameprofile.name,
                &chat_message,
            );
        }
    }

    /// Sends a system message to the player.
    pub fn send_message(&self, text: &TextComponent) {
        self.connection
            .send_packet(CSystemChatMessage::new(text, self, false));
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
        let awaiting = self.awaiting_position_from_client.lock();
        if let Some(pos) = *awaiting {
            let current_tick = self.tick_count.load(Ordering::Relaxed);
            let last_time = self.awaiting_teleport_time.load(Ordering::Relaxed);

            // Resend teleport after 20 ticks (~1 second) timeout
            if current_tick.wrapping_sub(last_time) > 20 {
                self.awaiting_teleport_time
                    .store(current_tick, Ordering::Relaxed);
                drop(awaiting);

                // Resend the teleport packet
                let (yaw, pitch) = self.rotation.load();
                let teleport_id = self.awaiting_teleport_id.load(Ordering::Relaxed);
                self.connection.send_packet(CPlayerPosition::absolute(
                    teleport_id,
                    pos.x,
                    pos.y,
                    pos.z,
                    yaw,
                    pitch,
                ));
            }
            return true; // Still awaiting, reject movement
        }

        self.awaiting_teleport_time
            .store(self.tick_count.load(Ordering::Relaxed), Ordering::Relaxed);
        false
    }

    /// Returns true if the player is in post-impulse grace period.
    fn is_in_post_impulse_grace_time(&self) -> bool {
        let current_tick = self.tick_count.load(Ordering::Relaxed);
        let last_impulse = self.last_impulse_tick.load(Ordering::Relaxed);
        current_tick.wrapping_sub(last_impulse) < movement::IMPULSE_GRACE_TICKS
    }

    /// Marks that an impulse (knockback, etc.) was applied to the player.
    pub fn apply_impulse(&self) {
        self.last_impulse_tick
            .store(self.tick_count.load(Ordering::Relaxed), Ordering::Relaxed);
    }

    /// Returns the squared length of the player's current velocity.
    fn get_delta_movement_length_sq(&self) -> f64 {
        let dm = self.delta_movement.lock();
        dm.x * dm.x + dm.y * dm.y + dm.z * dm.z
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
            self.connection
                .disconnect(translations::MULTIPLAYER_DISCONNECT_INVALID_PLAYER_MOVEMENT.msg());
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

        let prev_pos = *self.prev_position.lock();
        let prev_rot = self.prev_rotation.load();
        let start_pos = *self.position.lock();
        let game_mode = self.game_mode.load();
        let is_spectator = game_mode == GameType::Spectator;
        let is_creative = game_mode == GameType::Creative;
        let is_sleeping = self.sleeping.load(Ordering::Relaxed);
        let is_fall_flying = self.fall_flying.load(Ordering::Relaxed);
        let was_on_ground = self.on_ground.load(Ordering::Relaxed);
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
            let first_good = *self.first_good_position.lock();
            let last_good = *self.last_good_position.lock();

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
                // Increment received packet count
                self.received_move_packet_count
                    .fetch_add(1, Ordering::Relaxed);

                // Calculate delta packets since last tick (for rate limiting)
                let received = self.received_move_packet_count.load(Ordering::Relaxed);
                let known = self.known_move_packet_count.load(Ordering::Relaxed);
                let mut delta_packets = received - known;

                // Cap delta packets to prevent abuse (vanilla caps at 5)
                if delta_packets > 5 {
                    delta_packets = 1;
                }

                // Skip checks for spectators, creative mode, tick frozen, or gamerules disabled
                // Vanilla: shouldValidateMovement() checks playerMovementCheck and elytraMovementCheck
                let gamerule_skip = !self.should_validate_movement(is_fall_flying);
                let skip_checks = is_spectator || is_creative || tick_frozen || gamerule_skip;

                // Validate movement using physics simulation
                let mut validation = movement::validate_movement(
                    &self.world,
                    &movement::MovementInput {
                        target_pos,
                        first_good_pos: first_good,
                        last_good_pos: last_good,
                        expected_velocity_sq: self.get_delta_movement_length_sq(),
                        delta_packets,
                        is_fall_flying,
                        skip_checks,
                        in_impulse_grace: self.is_in_post_impulse_grace_time(),
                        is_crouching: self.shift_key_down.load(Ordering::Relaxed),
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
                *self.last_good_position.lock() = target_pos;

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
        self.on_ground.store(packet.on_ground, Ordering::Relaxed);

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
                let sync_delay = self.position_sync_delay.fetch_add(1, Ordering::Relaxed);
                let last_on_ground = self.last_sent_on_ground.load(Ordering::Relaxed);
                let on_ground_changed = last_on_ground != packet.on_ground;
                let force_sync = sync_delay > 400 || on_ground_changed;

                if let (Some(dx), Some(dy), Some(dz)) = (dx, dy, dz) {
                    if force_sync {
                        // Send absolute position sync (forced by timer or on_ground change)
                        self.position_sync_delay.store(0, Ordering::Relaxed);
                        self.last_sent_on_ground
                            .store(packet.on_ground, Ordering::Relaxed);

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
                    self.position_sync_delay.store(0, Ordering::Relaxed);
                    self.last_sent_on_ground
                        .store(packet.on_ground, Ordering::Relaxed);

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

            *self.prev_position.lock() = pos;
            self.prev_rotation.store((yaw, pitch));
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
                *self.chat_session.lock() = Some(session);
                *self.message_chain.lock() = Some(chain);
                return;
            }
        };

        *self.chat_session.lock() = Some(session);
        *self.message_chain.lock() = Some(chain);

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
        self.chat_session.lock().clone()
    }

    /// Checks if the player has a valid chat session
    pub fn has_chat_session(&self) -> bool {
        self.chat_session.lock().is_some()
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
                    self.connection.disconnect("Invalid profile public key");
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
                    self.connection
                        .disconnect(format!("Chat session validation failed: {err}"));
                }
            }
        }
    }

    /// Handles a chat acknowledgment packet from the client.
    pub fn handle_chat_ack(&self, packet: SChatAck) {
        if let Err(err) = self.message_validator.lock().apply_offset(packet.offset.0) {
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
            self.connection.send_packet(CSetChunkCacheRadius {
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

        self.game_mode.store(gamemode);

        // Update abilities based on new game mode (mirrors vanilla GameType.updatePlayerAbilities)
        self.abilities.lock().update_for_game_mode(gamemode);

        // Send abilities first (vanilla sends this before game event)
        self.send_abilities();

        self.connection.send_packet(CGameEvent {
            event: GameEventType::ChangeGameMode,
            data: gamemode.into(),
        });

        // Broadcast game mode update to all players (including self)
        // This updates PlayerInfo on clients, which is used for isSpectator() checks
        let update_packet =
            CPlayerInfoUpdate::update_game_mode(self.gameprofile.id, gamemode as i32);
        self.world.broadcast_to_all(update_packet);

        true
    }

    /// Sends the player abilities packet to the client.
    /// This tells the client about flight, invulnerability, speeds, etc.
    pub fn send_abilities(&self) {
        let packet = self.abilities.lock().to_packet();
        self.connection.send_packet(packet);
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
            self.connection.send_packet(CBlockChangedAck { sequence });
        }
    }

    /// Returns true if player is within block interaction range.
    /// Base range is ~4.5 blocks, plus 1.0 tolerance.
    #[must_use]
    pub fn is_within_block_interaction_range(&self, pos: &BlockPos) -> bool {
        let player_pos = *self.position.lock();
        let block_center_x = f64::from(pos.x()) + 0.5;
        let block_center_y = f64::from(pos.y()) + 0.5;
        let block_center_z = f64::from(pos.z()) + 0.5;

        // Base range is ~4.5 blocks, plus 1.0 tolerance
        let max_range = 4.5 + 1.0;
        let dx = player_pos.x - block_center_x;
        let dy = player_pos.y - block_center_y;
        let dz = player_pos.z - block_center_z;
        (dx * dx + dy * dy + dz * dz).sqrt() <= max_range
    }

    /// Returns true if player is sneaking (secondary use active).
    #[must_use]
    pub fn is_secondary_use_active(&self) -> bool {
        self.shift_key_down.load(Ordering::Relaxed)
    }

    /// Returns true if player has infinite materials (Creative mode).
    #[must_use]
    pub fn has_infinite_materials(&self) -> bool {
        self.game_mode.load() == GameType::Creative
    }

    /// Returns true if the player is currently sleeping.
    #[must_use]
    pub fn is_sleeping(&self) -> bool {
        self.sleeping.load(Ordering::Relaxed)
    }

    /// Sets the player's sleeping state.
    pub fn set_sleeping(&self, sleeping: bool) {
        self.sleeping.store(sleeping, Ordering::Relaxed);
    }

    /// Returns true if the player is currently fall flying (elytra).
    #[must_use]
    pub fn is_fall_flying(&self) -> bool {
        self.fall_flying.load(Ordering::Relaxed)
    }

    /// Sets the player's fall flying state.
    pub fn set_fall_flying(&self, fall_flying: bool) {
        self.fall_flying.store(fall_flying, Ordering::Relaxed);
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
        self.on_ground.load(Ordering::Relaxed)
    }

    /// Determines the desired pose based on current player state.
    /// Priority: `Sleeping` > `FallFlying` > `Sneaking` > `Standing`
    // TODO: Add Swimming pose (requires water detection)
    // TODO: Add SpinAttack pose (requires riptide trident)
    // TODO: Add pose collision checks (force crouch in low ceilings)
    fn get_desired_pose(&self) -> EntityPose {
        if self.sleeping.load(Ordering::Relaxed) {
            EntityPose::Sleeping
        } else if self.fall_flying.load(Ordering::Relaxed) {
            EntityPose::FallFlying
        } else if self.shift_key_down.load(Ordering::Relaxed) && !self.abilities.lock().flying {
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
        *self.delta_movement.lock()
    }

    /// Sets the player's velocity.
    pub fn set_delta_movement(&self, velocity: Vector3<f64>) {
        *self.delta_movement.lock() = velocity;
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
        let on_ground = self.on_ground.load(Ordering::Relaxed);
        let game_mode = self.game_mode.load();
        let is_spectator = game_mode == GameType::Spectator;
        let is_creative_flying = game_mode == GameType::Creative; // TODO: check actual flying state
        let is_fall_flying = self.fall_flying.load(Ordering::Relaxed);

        // Skip gravity when on ground, spectating, creative flying, or elytra flying
        if on_ground || is_spectator || is_creative_flying || is_fall_flying {
            return;
        }

        let gravity = self.get_gravity();
        if gravity != 0.0 {
            let mut dm = self.delta_movement.lock();
            dm.y -= gravity;
        }
    }

    /// Returns true if we're waiting for a teleport confirmation.
    #[must_use]
    pub fn is_awaiting_teleport(&self) -> bool {
        self.awaiting_position_from_client.lock().is_some()
    }

    /// Teleports the player to a new position.
    ///
    /// Sends a `CPlayerPosition` packet and waits for client acknowledgment.
    /// Until acknowledged, movement packets from the client will be rejected.
    ///
    /// Matches vanilla `ServerGamePacketListenerImpl.teleport()`.
    pub fn teleport(&self, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) {
        let current_tick = self.tick_count.load(Ordering::Relaxed);
        self.awaiting_teleport_time
            .store(current_tick, Ordering::Relaxed);

        // Pre-increment teleport ID, wrapping at i32::MAX (matches vanilla: ++awaitingTeleport)
        let new_id = self
            .awaiting_teleport_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
                Some(if id == i32::MAX { 0 } else { id + 1 })
            })
            .map_or(1, |old| if old == i32::MAX { 0 } else { old + 1 });

        // Update player position (vanilla: player.teleportSetPosition)
        *self.position.lock() = Vector3::new(x, y, z);
        self.rotation.store((yaw, pitch));

        // Store the position we're waiting for confirmation of
        // (vanilla stores player.position() after teleportSetPosition)
        *self.awaiting_position_from_client.lock() = Some(Vector3::new(x, y, z));

        // Send the teleport packet with the new ID
        self.connection
            .send_packet(CPlayerPosition::absolute(new_id, x, y, z, yaw, pitch));
    }

    /// Handles a teleport acknowledgment from the client.
    ///
    /// Matches vanilla `ServerGamePacketListenerImpl.handleAcceptTeleportPacket()`.
    pub fn handle_accept_teleportation(&self, packet: SAcceptTeleportation) {
        let expected_id = self.awaiting_teleport_id.load(Ordering::Relaxed);

        if packet.teleport_id == expected_id {
            let mut awaiting = self.awaiting_position_from_client.lock();
            if awaiting.is_none() {
                // Client sent confirmation without server sending teleport
                self.connection
                    .disconnect(translations::MULTIPLAYER_DISCONNECT_INVALID_PLAYER_MOVEMENT.msg());
                return;
            }

            // Snap player to awaited position (vanilla: player.absSnapTo)
            if let Some(pos) = *awaiting {
                *self.position.lock() = pos;
                *self.last_good_position.lock() = pos;
            }

            // Clear awaiting state
            *awaiting = None;
        }
        // If ID doesn't match, silently ignore (could be old/delayed packet)
    }

    /// Sends block update packets for a position and its neighbor.
    fn send_block_updates(&self, pos: &BlockPos, direction: Direction) {
        let state = self.world.get_block_state(pos);
        self.connection.send_packet(CBlockUpdate {
            pos: *pos,
            block_state: state,
        });

        let neighbor_pos = direction.relative(pos);
        let neighbor_state = self.world.get_block_state(&neighbor_pos);
        self.connection.send_packet(CBlockUpdate {
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
        self.shift_key_down.store(packet.shift(), Ordering::Relaxed);
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
        self.connection.send_packet(CSetHeldSlot {
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
        self.connection.send_packet(CBlockUpdate {
            pos,
            block_state: state,
        });

        // Then open the sign editor
        self.connection
            .send_packet(COpenSignEditor { pos, is_front_text });
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
        self.connection.send_packet(COpenScreen {
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
            self.connection.send_packet(CContainerClose {
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
            // First time being removed - notify callback
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
        *self.delta_movement.lock()
    }

    fn on_ground(&self) -> bool {
        self.on_ground.load(Ordering::Relaxed)
    }

    /// Returns the eye height for the current pose.
    ///
    /// Vanilla eye heights from `Avatar.POSES`:
    /// - Standing: 1.62
    /// - Crouching: 1.27
    /// - Swimming/FallFlying/SpinAttack: 0.4
    /// - Sleeping/Dying: 0.2
    fn get_eye_height(&self) -> f64 {
        match self.get_desired_pose() {
            EntityPose::Sneaking => 1.27,
            EntityPose::FallFlying | EntityPose::Swimming | EntityPose::SpinAttack => 0.4,
            EntityPose::Sleeping | EntityPose::Dying => 0.2,
            // Standing and all other poses use default player eye height
            _ => f64::from(vanilla_entities::PLAYER.dimensions.eye_height),
        }
    }
}

impl LivingEntity for Player {
    fn get_health(&self) -> f32 {
        *self.entity_data.lock().health.get()
    }

    fn set_health(&mut self, health: f32) {
        let max_health = self.get_max_health();
        let clamped = health.clamp(0.0, max_health);
        self.entity_data.lock().health.set(clamped);
        // Dirty flag set automatically, will sync on next tick
    }

    fn get_max_health(&self) -> f32 {
        // TODO: Get from attributes system when implemented
        20.0
    }

    fn get_position(&self) -> Vector3<f64> {
        *self.position.lock()
    }

    fn get_absorption_amount(&self) -> f32 {
        *self.entity_data.lock().player_absorption.get()
    }

    fn set_absorption_amount(&mut self, amount: f32) {
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
        self.sprinting.load(Ordering::Relaxed)
    }

    fn set_sprinting(&mut self, sprinting: bool) {
        self.sprinting.store(sprinting, Ordering::Relaxed);
        // TODO: Apply speed modifiers when attribute system is implemented
    }

    fn get_speed(&self) -> f32 {
        self.speed.load()
    }

    fn set_speed(&mut self, speed: f32) {
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
