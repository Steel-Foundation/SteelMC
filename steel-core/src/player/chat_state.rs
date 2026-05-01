//! Chat and messaging state for a player.
//!
//! Groups the fields related to secure chat: message counters, signature cache,
//! message validator, chat session, and message chain.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use steel_crypto::{SignatureValidator, public_key_from_bytes, signature::NoValidation};
use steel_protocol::packets::game::CSystemChatMessage;
use steel_protocol::packets::game::{
    CPlayerChat, CPlayerInfoUpdate, ChatTypeBound, FilterType, SChat, SChatAck, SChatSessionUpdate,
};
use steel_registry::{RegistryEntry, vanilla_chat_types};
use text_components::Modifier;
use text_components::TextComponent;
use text_components::interactivity::{ClickEvent, HoverEvent};

use super::LastSeenMessagesValidator;
use super::message_chain::SignedMessageChain;
use super::profile_key::RemoteChatSession;
use super::{LastSeen, MessageCache};
use crate::entity::Entity;
use crate::player::{Player, message_chain, profile_key};

/// All chat-related state for a player.
///
/// Stored behind a single `SyncMutex` on `Player`. The fields were previously
/// individual atomics/mutexes but are always accessed within short critical
/// sections per-player, so a single lock is simpler with no real contention cost.
pub struct ChatState {
    /// Counter for chat messages sent BY this player.
    pub messages_sent: i32,
    /// Counter for chat messages received BY this player.
    pub messages_received: i32,
    /// Message signature cache for tracking chat messages.
    pub signature_cache: MessageCache,
    /// Validator for client acknowledgements of messages we've sent.
    pub message_validator: LastSeenMessagesValidator,
    /// Remote chat session containing the player's public key (if signed chat is enabled).
    pub chat_session: Option<RemoteChatSession>,
    /// Message chain state for tracking signed message sequence.
    pub message_chain: Option<SignedMessageChain>,
}

impl ChatState {
    pub fn new() -> Self {
        Self {
            messages_sent: 0,
            messages_received: 0,
            signature_cache: MessageCache::new(),
            message_validator: LastSeenMessagesValidator::new(),
            chat_session: None,
            message_chain: None,
        }
    }
}

impl Player {
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
    pub fn handle_chat(&self, packet: SChat, player: Arc<Player>) {
        let chat_message = packet.message.clone();

        let verification_result = if let Some(_signature) = &packet.signature {
            match self.verify_chat_signature(&packet) {
                Ok((link, last_seen)) => Some(Ok((link, last_seen))),
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

        if self.config.enforce_secure_chat {
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

        let registry_id = vanilla_chat_types::CHAT.id() as i32;

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

        steel_utils::chat!(player.gameprofile.name.clone(), "{}", chat_message);
        if let Some(sig_box) = &signature
            && sig_box.len() == 256
        {
            let mut sig_array = [0u8; 256];
            sig_array.copy_from_slice(&sig_box[..]);

            let last_seen = if let Some(Ok((_, ref last_seen))) = verification_result {
                last_seen.clone()
            } else {
                LastSeen::default()
            };

            for world in self.server().worlds.values() {
                world.broadcast_chat(
                    chat_packet.clone(),
                    Arc::clone(&player),
                    last_seen.clone(),
                    Some(&sig_array),
                );
            }
        } else {
            for world in self.server().worlds.values() {
                world.broadcast_unsigned_chat(chat_packet.clone());
            }
        }
    }

    /// Sends a system message to the player.
    pub fn send_message(&self, text: &TextComponent) {
        self.send_packet(CSystemChatMessage::new(text, self, false));
    }

    /// Updates the player's chat session and initializes the message chain.
    ///
    /// This should be called when receiving a `ChatSessionUpdate` packet from the client.
    pub fn set_chat_session(&self, session: RemoteChatSession) {
        let chain = SignedMessageChain::new(self.gameprofile.id, session.session_id);

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

        let update_packet =
            CPlayerInfoUpdate::update_chat_session(self.gameprofile.id, protocol_data);
        self.get_world().broadcast_to_all(update_packet);
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

        let expires_at = UNIX_EPOCH + Duration::from_millis(packet.expires_at as u64);

        let public_key = match public_key_from_bytes(&packet.public_key) {
            Ok(key) => key,
            Err(err) => {
                log::warn!(
                    "Player {} sent invalid public key: {err}",
                    self.gameprofile.name
                );
                if self.config.enforce_secure_chat {
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
                if self.config.enforce_secure_chat {
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
}
