//! This module contains all things player-related.
pub mod block_breaking;
pub mod chunk_sender;
mod game_mode;
mod game_profile;
pub mod message_chain;
mod message_validator;
pub mod movement;
/// This module contains the networking implementation for the player.
pub mod networking;
pub mod player_inventory;
pub mod profile_key;
mod signature_cache;

use std::{
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use block_breaking::BlockBreakingManager;
use crossbeam::atomic::AtomicCell;
pub use game_profile::GameProfile;
use message_chain::SignedMessageChain;
use message_validator::LastSeenMessagesValidator;
use profile_key::RemoteChatSession;
pub use signature_cache::{LastSeen, MessageCache};
use steel_protocol::packet_traits::EncodedPacket;
use steel_protocol::packets::game::CSetHeldSlot;
use steel_protocol::packets::game::{
    AnimateAction, CAnimate, CPlayerPosition, PlayerAction, SAcceptTeleportation,
    SPickItemFromBlock, SPlayerAction, SSetCarriedItem, SUseItem, SUseItemOn,
};
use steel_protocol::utils::ConnectionProtocol;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::game_rules::GameRuleValue;
use steel_registry::vanilla_game_rules::{ELYTRA_MOVEMENT_CHECK, PLAYER_MOVEMENT_CHECK};

use steel_utils::locks::SyncMutex;
use steel_utils::types::GameType;

use crate::config::STEEL_CONFIG;
use crate::inventory::SyncPlayerInv;
use crate::player::player_inventory::PlayerInventory;

use steel_crypto::{SignatureValidator, public_key_from_bytes, signature::NoValidation};
use steel_protocol::packets::{
    common::{SClientInformation, SCustomPayload},
    game::{
        CBlockChangedAck, CBlockUpdate, CContainerClose, CGameEvent, CMoveEntityPosRot,
        CMoveEntityRot, COpenScreen, CPlayerChat, CPlayerInfoUpdate, CRotateHead,
        CSetChunkCacheRadius, ChatTypeBound, FilterType, GameEventType, PreviousMessage, SChat,
        SChatAck, SChatSessionUpdate, SContainerButtonClick, SContainerClick, SContainerClose,
        SContainerSlotStateChanged, SMovePlayer, SPlayerInput, SSetCreativeModeSlot, calc_delta,
        to_angle_byte,
    },
};
use steel_registry::{blocks::properties::Direction, item_stack::ItemStack};

use crate::behavior::{BLOCK_BEHAVIORS, InteractionResult};
use steel_utils::BlockPos;

use steel_utils::types::InteractionHand;
use steel_utils::{ChunkPos, math::Vector3, text::TextComponent, translations};

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

    /// The entity ID assigned to this player.
    pub entity_id: i32,

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

    // LivingEntity fields
    /// The player's health (synced with client via entity data).
    health: AtomicCell<f32>,
    /// The player's absorption amount (extra health from effects like Absorption).
    absorption_amount: AtomicCell<f32>,
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

    /// Whether the player is currently fall flying (elytra gliding).
    fall_flying: AtomicBool,

    /// Whether the player is on the ground.
    on_ground: AtomicBool,

    /// Tick when last impulse was applied (knockback, etc.).
    /// Used for post-impulse grace period during movement validation.
    last_impulse_tick: AtomicI32,

    /// Block breaking state machine.
    pub block_breaking: SyncMutex<BlockBreakingManager>,
}

impl Player {
    /// Creates a new player.
    pub fn new(
        gameprofile: GameProfile,
        connection: Arc<JavaConnection>,
        world: Arc<World>,
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
            entity_id,
            client_loaded: AtomicBool::new(false),
            position: SyncMutex::new(pos),
            rotation: AtomicCell::new((0.0, 0.0)),
            prev_position: SyncMutex::new(pos),
            prev_rotation: AtomicCell::new((0.0, 0.0)),
            health: AtomicCell::new(20.0), // Default max health
            absorption_amount: AtomicCell::new(0.0),
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
            fall_flying: AtomicBool::new(false),
            on_ground: AtomicBool::new(false),
            last_impulse_tick: AtomicI32::new(0),
            block_breaking: SyncMutex::new(BlockBreakingManager::new()),
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

        // Broadcast inventory changes to client
        self.broadcast_inventory_changes();

        // Tick block breaking
        self.block_breaking.lock().tick(self, &self.world);

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

    /// Handles a custom payload packet.
    pub fn handle_custom_payload(&self, packet: SCustomPayload) {
        log::info!("Hello from the other side! {packet:?}");
    }

    /// Handles the end of a client tick.
    pub fn handle_client_tick_end(&self) {
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
        const MESSAGE_EXPIRES_AFTER: Duration = Duration::from_secs(5 * 60);

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
                    self.connection.disconnect(
                        TextComponent::new().text(format!("Chat message validation failed: {err}")),
                    );
                    return;
                }
                None => {
                    self.connection.disconnect(TextComponent::new().text(
                        "Secure chat is enforced on this server, but your message was not signed",
                    ));
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

        let chat_packet = CPlayerChat::new(
            0,
            player.gameprofile.id,
            sender_index,
            signature.clone(),
            chat_message.clone(),
            packet.timestamp,
            packet.salt,
            Box::new([]),
            Some(TextComponent::new().text(chat_message.clone())),
            FilterType::PassThrough,
            ChatTypeBound {
                //TODO: Use the registry to derive this instead of hardcoding it
                registry_id: 0,
                sender_name: TextComponent::new().text(player.gameprofile.name.clone()),
                target_name: None,
            },
        );

        if let Some(ref sig_box) = signature {
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

    fn is_invalid_position(x: f64, y: f64, z: f64, rot_x: f32, rot_y: f32) -> bool {
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
                let validation = movement::validate_movement(
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

                // Update velocity based on actual movement (vanilla: handlePlayerKnownMovement)
                {
                    let mut dm = self.delta_movement.lock();
                    dm.x = validation.move_delta.x;
                    dm.y = validation.move_delta.y;
                    dm.z = validation.move_delta.z;

                    // Zero Y velocity when landing (vanilla: Block.updateEntityMovementAfterFallOn)
                    // This prevents gravity from accumulating while on the ground
                    if !was_on_ground && packet.on_ground {
                        dm.y = 0.0;
                    }
                }

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
            *self.position.lock() = packet.position;
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
                let move_packet = CMoveEntityPosRot {
                    entity_id: self.entity_id,
                    dx: calc_delta(pos.x, prev_pos.x),
                    dy: calc_delta(pos.y, prev_pos.y),
                    dz: calc_delta(pos.z, prev_pos.z),
                    y_rot: to_angle_byte(yaw),
                    x_rot: to_angle_byte(pitch),
                    on_ground: packet.on_ground,
                };
                if let Ok(encoded) = EncodedPacket::from_bare(
                    move_packet,
                    STEEL_CONFIG.compression,
                    ConnectionProtocol::Play,
                ) {
                    self.world
                        .broadcast_to_nearby(new_chunk, encoded, Some(self.entity_id));
                }
            } else {
                let rot_packet = CMoveEntityRot {
                    entity_id: self.entity_id,
                    y_rot: to_angle_byte(yaw),
                    x_rot: to_angle_byte(pitch),
                    on_ground: packet.on_ground,
                };
                if let Ok(encoded) = EncodedPacket::from_bare(
                    rot_packet,
                    STEEL_CONFIG.compression,
                    ConnectionProtocol::Play,
                ) {
                    self.world
                        .broadcast_to_nearby(new_chunk, encoded, Some(self.entity_id));
                }
            }

            if packet.has_rot {
                let head_packet = CRotateHead {
                    entity_id: self.entity_id,
                    head_y_rot: to_angle_byte(yaw),
                };
                if let Ok(encoded) = EncodedPacket::from_bare(
                    head_packet,
                    STEEL_CONFIG.compression,
                    ConnectionProtocol::Play,
                ) {
                    self.world
                        .broadcast_to_nearby(new_chunk, encoded, Some(self.entity_id));
                }
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

        self.world.players.iter_players(|_, player| {
            player.connection.send_packet(update_packet.clone());
            true
        });
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
                    self.connection
                        .disconnect(TextComponent::new().text("Invalid profile public key"));
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
                    self.connection.disconnect(
                        TextComponent::new().text(format!("Chat session validation failed: {err}")),
                    );
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

        self.connection.send_packet(CGameEvent {
            event: GameEventType::ChangeGameMode,
            data: gamemode.into(),
        });

        true
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

    /// Returns true if the player is on the ground.
    #[must_use]
    pub fn is_on_ground(&self) -> bool {
        self.on_ground.load(Ordering::Relaxed)
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
    fn get_gravity(&self) -> f64 {
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
            .map(|old| if old == i32::MAX { 0 } else { old + 1 })
            .unwrap_or(1);

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
        let packet = CAnimate::new(self.entity_id, action);

        let chunk = *self.last_chunk_pos.lock();
        let exclude = if update_self {
            None
        } else {
            Some(self.entity_id)
        };
        if let Ok(encoded) =
            EncodedPacket::from_bare(packet, STEEL_CONFIG.compression, ConnectionProtocol::Play)
        {
            self.world.broadcast_to_nearby(chunk, encoded, exclude);
        }
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
                // TODO: Implement drop all items (Q + Ctrl)
                log::debug!("Player {} wants to drop all items", self.gameprofile.name);
            }
            PlayerAction::DropItem => {
                // TODO: Implement drop single item (Q)
                log::debug!("Player {} wants to drop an item", self.gameprofile.name);
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
        if let Some(ref menu) = *open_menu {
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

    /// Drops an item into the world.
    ///
    /// Based on Java's `Player.drop(ItemStack, boolean throwRandomly)`.
    ///
    /// - `throw_randomly`: If true, the item is thrown in a random direction (like pressing Q).
    ///   If false, it's thrown in the direction the player is facing.
    pub fn drop_item(&self, item: ItemStack, throw_randomly: bool) {
        if item.is_empty() {
            return;
        }
        // TODO: Spawn an ItemEntity in the world at the player's position
        // For now, just log it
        log::debug!(
            "Player {} dropped item: {:?} (throw_randomly: {})",
            self.gameprofile.name,
            item,
            throw_randomly
        );
    }

    /// Returns true if the player can drop items.
    ///
    /// Based on Java's `Player.canDropItems()`.
    /// Returns false if the player is dead, removed, or has a flag preventing item drops.
    #[must_use]
    pub fn can_drop_items(&self) -> bool {
        // TODO: Check if player is alive and not removed
        // For now, always return true
        true
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
            self.drop_item(item, false);
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
                self.drop_item(item, false);
            }
        } else {
            // Inventory not in guard - this shouldn't happen but drop the item to be safe
            self.drop_item(item, false);
        }
    }

    /// Cleans up player resources.
    pub fn cleanup(&self) {}
}

impl LivingEntity for Player {
    fn get_health(&self) -> f32 {
        self.health.load()
    }

    fn set_health(&mut self, health: f32) {
        let max_health = self.get_max_health();
        let clamped = health.clamp(0.0, max_health);
        self.health.store(clamped);
        // TODO: Sync health to client via entity data
    }

    fn get_max_health(&self) -> f32 {
        // TODO: Get from attributes system when implemented
        20.0
    }

    fn get_position(&self) -> Vector3<f64> {
        *self.position.lock()
    }

    fn get_absorption_amount(&self) -> f32 {
        self.absorption_amount.load()
    }

    fn set_absorption_amount(&mut self, amount: f32) {
        self.absorption_amount.store(amount.max(0.0));
        // TODO: Sync to client
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
