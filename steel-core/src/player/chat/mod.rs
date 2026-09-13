//! Chat and messaging state for a player.
//!
//! Groups the fields related to secure chat: message counters, signature cache,
//! message validator, chat session, and message chain.

pub mod message_chain;
mod message_validator;
pub mod profile_key;
mod signature_cache;

pub use message_validator::LastSeenMessagesValidator;
pub use signature_cache::{LastSeen, MessageCache};

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use steel_crypto::{SignatureValidator, public_key_from_bytes};
use steel_protocol::packets::game::{
    CDisguisedChat, CPlayerChat, CPlayerInfoUpdate, CSystemChat, ChatTypeBound, FilterType, SChat,
    SChatAck, SChatCommandSigned, SChatSessionUpdate,
};
use steel_registry::{RegistryEntry, vanilla_chat_types};
use steel_utils::translations;
use text_components::Modifier;
use text_components::TextComponent;
use text_components::format::Color;
use text_components::interactivity::{ClickEvent, HoverEvent};

use crate::command::sender::CommandSender;
use crate::command::signing_context::CommandSigningContext;
use crate::entity::Entity;
use crate::player::Player;
use crate::player::spam_throttler::TickThrottler;
use crate::server::Server;
use message_chain::SignedMessageChain;
use profile_key::RemoteChatSession;
use steel_utils::translations::{
    CHAT_DISABLED_CHAIN_BROKEN, CHAT_DISABLED_EXPIRED_PROFILE_KEY, CHAT_DISABLED_INVALID_SIGNATURE,
    CHAT_DISABLED_MISSING_PROFILE_KEY, CHAT_DISABLED_OUT_OF_ORDER_CHAT,
    MULTIPLAYER_DISCONNECT_CHAT_VALIDATION_FAILED, MULTIPLAYER_DISCONNECT_ILLEGAL_CHARACTERS,
};

/// Vanilla `PlayerChatMessage.MESSAGE_EXPIRES_AFTER_SERVER`.
const MESSAGE_EXPIRES_AFTER_SERVER: Duration = Duration::from_mins(5);

/// All chat-related state for a player.
///
/// Stored behind a single `SyncMutex` on `PlayerSession`. The fields were previously
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
    chat_spam_throttler: TickThrottler,
    command_spam_throttler: TickThrottler,
}

enum ChatSessionUpdateOutcome {
    Unchanged,
    MissingServiceKeys,
    ExpiryDowngrade,
    Accepted(RemoteChatSession),
    Invalid(profile_key::ValidationError),
}

fn validate_chat_session_update(
    old_profile_key: Option<&profile_key::ProfilePublicKeyData>,
    new_session: profile_key::RemoteChatSessionData,
    profile_id: uuid::Uuid,
    validator: Option<&dyn SignatureValidator>,
) -> ChatSessionUpdateOutcome {
    if old_profile_key == Some(&new_session.profile_public_key) {
        return ChatSessionUpdateOutcome::Unchanged;
    }
    if old_profile_key
        .is_some_and(|old_key| new_session.profile_public_key.expires_at < old_key.expires_at)
    {
        return ChatSessionUpdateOutcome::ExpiryDowngrade;
    }
    let Some(validator) = validator else {
        return ChatSessionUpdateOutcome::MissingServiceKeys;
    };

    match new_session.validate(profile_id, validator) {
        Ok(session) => ChatSessionUpdateOutcome::Accepted(session),
        Err(error) => ChatSessionUpdateOutcome::Invalid(error),
    }
}

impl ChatState {
    /// Creates empty chat state with the configured Vanilla spam thresholds.
    #[must_use]
    pub fn new(chat_spam_threshold_seconds: i32, command_spam_threshold_seconds: i32) -> Self {
        Self {
            messages_sent: 0,
            messages_received: 0,
            signature_cache: MessageCache::new(),
            message_validator: LastSeenMessagesValidator::new(),
            chat_session: None,
            message_chain: None,
            chat_spam_throttler: TickThrottler::new(
                20,
                chat_spam_threshold_seconds.wrapping_mul(20),
            ),
            command_spam_throttler: TickThrottler::new(
                20,
                command_spam_threshold_seconds.wrapping_mul(20),
            ),
        }
    }
}

pub enum OutgoingChatMessage {
    /// Signed or unsigned chat message from a genuine player session
    Player {
        packet: CPlayerChat,
        signature: Option<[u8; 256]>,
        sender_last_seen: LastSeen,
    },
    /// Unsigned message (console, command block)
    Disguised { content: TextComponent },
}

impl OutgoingChatMessage {
    /// Determines whether to track as a signed player packet or disguised system packet.
    pub fn create(
        content: TextComponent,
        player_chat_data: Option<(CPlayerChat, Option<[u8; 256]>, LastSeen)>,
    ) -> Self {
        match player_chat_data {
            Some((packet, signature, sender_last_seen)) => Self::Player {
                packet,
                signature,
                sender_last_seen,
            },
            None => Self::Disguised { content },
        }
    }

    /// Dispatches the appropriate packet to the given recipient.
    pub fn send_to_player(&self, recipient: &Player, chat_type: &ChatTypeBound) {
        match self {
            Self::Player {
                packet,
                signature,
                sender_last_seen,
            } => {
                let mut packet = packet.clone();
                let messages_received = recipient.get_and_increment_messages_received();
                packet.global_index = messages_received;

                // Override with the contextual chat type (e.g., SAY_COMMAND)
                packet.chat_type = chat_type.clone();

                log::debug!(
                    "Broadcasting to player {} (UUID: {}), global_index={}",
                    recipient.gameprofile.name,
                    recipient.gameprofile.id,
                    messages_received
                );

                // IMPORTANT: Index previous messages BEFORE updating the cache
                // This matches vanilla's order: pack() then push()
                let previous_messages = {
                    let chat = recipient.chat().lock();
                    chat.signature_cache
                        .index_previous_messages(sender_last_seen)
                };
                packet.previous_messages.clone_from(&previous_messages);

                // Send the packet
                recipient.send_packet(packet);

                // AFTER sending, update the recipient's cache using vanilla's push algorithm
                // This adds all lastSeen signatures + current signature to the cache
                {
                    let mut chat = recipient.chat().lock();
                    if let Some(signature) = signature {
                        chat.signature_cache
                            .push(&sender_last_seen, Some(signature));

                        log::debug!("  Added signature to recipient's cache and pending list");

                        // Add to pending messages for acknowledgment tracking
                        chat.message_validator
                            .add_pending(Some(Box::new(*signature) as Box<[u8]>));
                    } else {
                        // Even unsigned messages update the pending tracker
                        chat.message_validator.add_pending(None);
                        log::debug!("  Added unsigned message to pending list");
                    }
                }
            }
            Self::Disguised { content } => {
                let packet = CDisguisedChat::new(content, chat_type.clone(), recipient);
                recipient.send_packet(packet);
            }
        }
    }
}

impl Player {
    /// Decays the throttlers of the player once per server tick.
    pub fn tick_throttlers(&self) {
        let mut chat = self.chat().lock();
        chat.chat_spam_throttler.tick();
        chat.command_spam_throttler.tick();
        drop(chat);

        self.session.drop_spam_throttler.lock().tick();
    }

    const fn should_disconnect_for_rate_spam(
        throttler: &mut TickThrottler,
        is_operator: bool,
    ) -> bool {
        throttler.increment();
        // TODO: Also exempt the singleplayer owner once Steel models that state.
        !throttler.is_under_threshold() && !is_operator
    }

    /// Applies Vanilla command spam accounting after a command is handled
    pub fn detect_command_rate_spam(&self) {
        let is_operator = self.is_operator();
        let should_disconnect = {
            let mut chat = self.chat().lock();
            Self::should_disconnect_for_rate_spam(&mut chat.command_spam_throttler, is_operator)
        };

        if should_disconnect {
            self.disconnect(translations::DISCONNECT_SPAM.msg());
        }
    }

    fn detect_chat_rate_spam(&self) {
        let is_operator = self.is_operator();
        let should_disconnect = {
            let mut chat = self.chat().lock();
            Self::should_disconnect_for_rate_spam(&mut chat.chat_spam_throttler, is_operator)
        };

        if should_disconnect {
            self.disconnect(translations::DISCONNECT_SPAM.msg());
        }
    }

    /// Gets the next `messages_received` counter and increments it
    pub fn get_and_increment_messages_received(&self) -> i32 {
        let mut chat = self.chat().lock();
        let val = chat.messages_received;
        chat.messages_received += 1;
        val
    }

    // Verify signature, advance chain, and handle breaking on error
    fn verify_and_advance_chain(
        chat: &mut ChatState,
        session: &RemoteChatSession,
        content: &str,
        timestamp: u64,
        salt: i64,
        last_seen: LastSeen,
        signature: &[u8; 256],
    ) -> Result<message_chain::SignedMessageLink, TextComponent> {
        let chain = chat
            .message_chain
            .as_mut()
            .ok_or_else(|| CHAT_DISABLED_MISSING_PROFILE_KEY.msg().component())?;

        if chain.is_broken() {
            return Err(CHAT_DISABLED_CHAIN_BROKEN.msg().component());
        }

        let message_time = UNIX_EPOCH + Duration::from_millis(timestamp);
        let now = SystemTime::now();
        let message_age = now.duration_since(message_time).unwrap_or(Duration::ZERO);

        if message_age > MESSAGE_EXPIRES_AFTER_SERVER {
            return Err(TextComponent::plain(format!(
                "Message expired (age: {}s, max: {}s)",
                message_age.as_secs(),
                MESSAGE_EXPIRES_AFTER_SERVER.as_secs()
            )));
        }

        let body = message_chain::SignedMessageBody::new(
            content.to_string(),
            message_time,
            salt,
            last_seen,
        );

        let link = chain.validate_and_advance(&body).map_err(|err| match err {
            message_chain::ChainError::OutOfOrderChat => {
                CHAT_DISABLED_OUT_OF_ORDER_CHAT.msg().component()
            }
            message_chain::ChainError::ChainBroken => CHAT_DISABLED_CHAIN_BROKEN.msg().component(),
            message_chain::ChainError::ExpiredProfileKey => {
                CHAT_DISABLED_EXPIRED_PROFILE_KEY.msg().component()
            }
            message_chain::ChainError::MissingProfileKey => {
                CHAT_DISABLED_MISSING_PROFILE_KEY.msg().component()
            }
            _ => TextComponent::plain(format!("Chain validation failed: {err}")),
        })?;

        let updater = message_chain::MessageSignatureUpdater::new(&link, &body);
        let validator = session.profile_public_key.create_signature_validator();

        match SignatureValidator::validate(&validator, &updater, signature) {
            Ok(true) => Ok(link),
            Ok(false) => {
                chain.break_chain();
                Err(CHAT_DISABLED_INVALID_SIGNATURE.msg().component())
            }
            Err(err) => {
                log::error!("Signature cryptographic evaluation failed: {err}");
                chain.break_chain();
                Err(CHAT_DISABLED_INVALID_SIGNATURE.msg().component())
            }
        }
    }

    fn verify_chat_signature(
        &self,
        packet: &SChat,
    ) -> Result<(message_chain::SignedMessageLink, LastSeen), TextComponent> {
        let mut chat = self.chat().lock();
        let session = chat
            .chat_session
            .clone()
            .ok_or_else(|| CHAT_DISABLED_MISSING_PROFILE_KEY.msg().component())?;

        let signature = packet
            .signature
            .as_ref()
            .ok_or_else(|| CHAT_DISABLED_MISSING_PROFILE_KEY.msg().component())?;

        if session
            .profile_public_key
            .data()
            .has_expired_with_grace(profile_key::EXPIRY_GRACE_PERIOD)
        {
            return Err(CHAT_DISABLED_EXPIRED_PROFILE_KEY.msg().component());
        }

        let last_seen_signatures = chat
            .message_validator
            .apply_update(packet.acknowledged, packet.offset, packet.checksum)
            .map_err(|e| {
                log::error!("Message acknowledgment validation failed: {e}");
                MULTIPLAYER_DISCONNECT_CHAT_VALIDATION_FAILED
                    .msg()
                    .component()
            })?;

        let last_seen = LastSeen::new(last_seen_signatures);

        let link = Self::verify_and_advance_chain(
            &mut chat,
            &session,
            &packet.message,
            packet.timestamp.try_into().unwrap_or(0),
            packet.salt,
            last_seen.clone(),
            signature,
        )?;

        Ok((link, last_seen))
    }
    /// Handles a chat message from the player.
    pub fn handle_chat(&self, packet: SChat, player: Arc<Player>) {
        player.reset_last_action_time();
        let chat_message = packet.message.clone();

        let verification_result = if let Some(_signature) = &packet.signature {
            match self.verify_chat_signature(&packet) {
                Ok((link, last_seen)) => Some(Ok((link, last_seen))),
                Err(err) => {
                    log::warn!(
                        "Failed to update secure chat state for {}: '{}'",
                        self.gameprofile.name,
                        err.clone().color(Color::Red)
                    );
                    Some(Err(err))
                }
            }
        } else {
            None
        };

        if self.server().enforces_secure_chat() {
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

        let sender_index = match &verification_result {
            Some(Ok((link, _))) => link.index,
            _ => 0,
        };

        let registry_id = vanilla_chat_types::CHAT.id() as i32;

        let chat_type = ChatTypeBound {
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
        };

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
            chat_type.clone(),
        );

        steel_utils::chat!(player.gameprofile.name.clone(), "{}", chat_message);

        let (signature, last_seen) = if let Some(sig_box) = &signature
            && sig_box.len() == 256
        {
            let mut sig_array = [0u8; 256];
            sig_array.copy_from_slice(&sig_box[..]);

            let last_seen = if let Some(Ok((_, ref last_seen))) = verification_result {
                last_seen.clone()
            } else {
                LastSeen::default()
            };

            (Some(sig_array), last_seen)
        } else {
            (None, LastSeen::default())
        };

        let outgoing = OutgoingChatMessage::Player {
            packet: chat_packet,
            signature,
            sender_last_seen: last_seen,
        };

        for world in self.server().worlds.values() {
            world.broadcast_chat(&outgoing, &chat_type);
        }

        self.detect_chat_rate_spam();
    }

    /// Sends a system message to the player.
    pub fn send_message(&self, text: &TextComponent) {
        self.send_packet(CSystemChat::new(text, false, self));
    }

    /// Sends an overlay system message to the player
    pub fn send_overlay_message(&self, text: &TextComponent) {
        self.send_packet(CSystemChat::new(text, true, self));
    }

    /// Sends vanilla's red upper build-height limit overlay.
    pub(crate) fn send_build_limit_too_high_message(&self, limit: i32) {
        let limit = TextComponent::plain(limit.to_string());
        let message = translations::BUILD_TOO_HIGH
            .message([limit])
            .color(Color::Red);
        self.send_overlay_message(&message);
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
                let mut chat = self.chat().lock();
                chat.chat_session = Some(session);
                chat.message_chain = Some(chain);
                return;
            }
        };

        {
            let mut chat = self.chat().lock();
            chat.chat_session = Some(session);
            chat.message_chain = Some(chain);
        }

        log::info!(
            "Player {} initialized signed chat session",
            self.gameprofile.name
        );

        let update_packet =
            CPlayerInfoUpdate::update_chat_session(self.gameprofile.id, protocol_data);
        self.server().broadcast_to_online(update_packet);
    }

    /// Gets a reference to the player's chat session if present
    pub fn chat_session(&self) -> Option<RemoteChatSession> {
        self.chat().lock().chat_session.clone()
    }

    /// Checks if the player has a valid chat session
    pub fn has_chat_session(&self) -> bool {
        self.chat().lock().chat_session.is_some()
    }

    /// Handles a chat session update packet from the client.
    ///
    /// This validates the player's profile key and initializes signed chat if valid.
    pub fn handle_chat_session_update(&self, packet: SChatSessionUpdate) {
        log::info!("Player {} sent chat session update", self.gameprofile.name);

        let expires_at = profile_key::system_time_from_millis(packet.expires_at);

        let public_key = match public_key_from_bytes(&packet.public_key) {
            Ok(key) => key,
            Err(err) => {
                log::warn!(
                    "Player {} sent invalid public key: {err}",
                    self.gameprofile.name
                );
                self.disconnect(
                    translations::MULTIPLAYER_DISCONNECT_INVALID_PUBLIC_KEY_SIGNATURE.msg(),
                );
                return;
            }
        };

        let profile_key_data =
            profile_key::ProfilePublicKeyData::new(expires_at, public_key, packet.key_signature);

        let session_data = profile_key::RemoteChatSessionData {
            session_id: packet.session_id,
            profile_public_key: profile_key_data,
        };

        let old_profile_key = self
            .chat_session()
            .map(|session| session.profile_public_key.data().clone());
        let validator = self.server().profile_key_signature_validator();
        match validate_chat_session_update(
            old_profile_key.as_ref(),
            session_data,
            self.gameprofile.id,
            validator
                .as_deref()
                .map(|validator| validator as &dyn SignatureValidator),
        ) {
            ChatSessionUpdateOutcome::Unchanged => {}
            ChatSessionUpdateOutcome::MissingServiceKeys => {
                log::warn!(
                    "Ignoring chat session from {} due to missing services public key",
                    self.gameprofile.name
                );
            }
            ChatSessionUpdateOutcome::ExpiryDowngrade => {
                self.disconnect(translations::MULTIPLAYER_DISCONNECT_EXPIRED_PUBLIC_KEY.msg());
            }
            ChatSessionUpdateOutcome::Accepted(session) => self.set_chat_session(session),
            ChatSessionUpdateOutcome::Invalid(error) => {
                log::warn!(
                    "Player {} sent invalid chat session: {error}",
                    self.gameprofile.name
                );
                self.disconnect(
                    translations::MULTIPLAYER_DISCONNECT_INVALID_PUBLIC_KEY_SIGNATURE.msg(),
                );
            }
        }
    }

    /// Handles a chat acknowledgment packet from the client.
    pub fn handle_chat_ack(&self, packet: SChatAck) {
        if let Err(err) = self
            .chat()
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

    pub fn handle_signed_command(
        self: &Arc<Self>,
        packet: SChatCommandSigned,
        server: &Arc<Server>,
    ) {
        // Check allow char
        for char in packet.command.chars() {
            let cp = char as u32;
            if !(cp >= 32 && cp != 127 && cp != 167) {
                self.disconnect(MULTIPLAYER_DISCONNECT_ILLEGAL_CHARACTERS.msg());
                return;
            }
        }

        let (last_seen, sender_index) = {
            let mut chat = self.chat().lock();

            let last_seen_sigs = match chat.message_validator.apply_update(
                packet.last_seen.acknowledged,
                packet.last_seen.offset.0,
                0,
            ) {
                Ok(signatures) => LastSeen::new(signatures),
                Err(error) => {
                    log::error!(
                        "Failed to validate message acknowledgements from {}: {}",
                        self.name(),
                        error
                    );
                    drop(chat);
                    self.disconnect(MULTIPLAYER_DISCONNECT_CHAT_VALIDATION_FAILED.msg());
                    return;
                }
            };

            let session = match chat.chat_session.clone() {
                Some(s) => s,
                None => {
                    drop(chat);
                    self.disconnect(CHAT_DISABLED_MISSING_PROFILE_KEY.msg());
                    return;
                }
            };

            let argument_value = packet
                .command
                .split_once(' ')
                .map(|(_, arg)| arg)
                .unwrap_or("");

            let mut sender_index = 0;
            for entry in &packet.argument_signatures {
                match Self::verify_and_advance_chain(
                    &mut chat,
                    &session,
                    argument_value,
                    packet.timestamp as u64,
                    packet.salt,
                    last_seen_sigs.clone(),
                    &entry.signature,
                ) {
                    Ok(link) => {
                        sender_index = link.index;
                    }
                    Err(err_component) => {
                        drop(chat);
                        self.send_message(&err_component);
                        return;
                    }
                }
            }

            (last_seen_sigs, sender_index)
        };

        self.reset_last_action_time();

        let signing_context = CommandSigningContext::new(
            packet.timestamp as u64,
            packet.salt,
            packet
                .argument_signatures
                .into_iter()
                .map(|entry| (entry.name, Box::from(entry.signature))),
            last_seen,
            sender_index,
        );

        if server
            .submit_command(
                CommandSender::Player(Arc::clone(self)),
                packet.command,
                Some(signing_context),
            )
            .is_err()
        {
            self.send_message(
                &TextComponent::const_plain("Command queue is full").color(Color::Red),
            );
        }

        self.detect_command_rate_spam();
    }
}

#[cfg(test)]
mod tests {
    use steel_crypto::{
        CryptError, SignatureValidator, generate_key_pair, signature::SignatureUpdater,
    };
    use uuid::Uuid;

    use super::{
        ChatSessionUpdateOutcome, ChatState, Player, profile_key, validate_chat_session_update,
    };

    struct FixedValidator(bool);

    impl SignatureValidator for FixedValidator {
        fn validate(
            &self,
            _updater: &dyn SignatureUpdater,
            _signature: &[u8],
        ) -> Result<bool, CryptError> {
            Ok(self.0)
        }
    }

    fn session(expires_at_millis: i64) -> profile_key::RemoteChatSessionData {
        let (_, public_key) = generate_key_pair().expect("test player key should generate");
        profile_key::RemoteChatSessionData {
            session_id: Uuid::new_v4(),
            profile_public_key: profile_key::ProfilePublicKeyData::new(
                profile_key::system_time_from_millis(expires_at_millis),
                public_key,
                vec![1],
            ),
        }
    }

    #[test]
    fn operators_are_exempt_from_both_spam_disconnects() {
        let mut chat = ChatState::new(1, 1);

        assert!(!Player::should_disconnect_for_rate_spam(
            &mut chat.command_spam_throttler,
            true,
        ));
        assert!(!Player::should_disconnect_for_rate_spam(
            &mut chat.chat_spam_throttler,
            true,
        ));
    }

    #[test]
    fn non_operators_still_trigger_both_spam_disconnects() {
        let mut chat = ChatState::new(1, 1);

        assert!(Player::should_disconnect_for_rate_spam(
            &mut chat.command_spam_throttler,
            false,
        ));
        assert!(Player::should_disconnect_for_rate_spam(
            &mut chat.chat_spam_throttler,
            false,
        ));
    }

    #[test]
    fn unchanged_profile_key_does_not_reset_the_session() {
        let current = session(2);
        let new_session = profile_key::RemoteChatSessionData {
            session_id: Uuid::new_v4(),
            profile_public_key: current.profile_public_key.clone(),
        };

        assert!(matches!(
            validate_chat_session_update(
                Some(&current.profile_public_key),
                new_session,
                Uuid::new_v4(),
                None,
            ),
            ChatSessionUpdateOutcome::Unchanged
        ));
    }

    #[test]
    fn expiry_downgrade_precedes_service_key_availability() {
        let current = session(2);

        assert!(matches!(
            validate_chat_session_update(
                Some(&current.profile_public_key),
                session(1),
                Uuid::new_v4(),
                None,
            ),
            ChatSessionUpdateOutcome::ExpiryDowngrade
        ));
    }

    #[test]
    fn missing_service_keys_ignore_new_session() {
        assert!(matches!(
            validate_chat_session_update(None, session(1), Uuid::new_v4(), None),
            ChatSessionUpdateOutcome::MissingServiceKeys
        ));
    }

    #[test]
    fn service_signature_result_controls_session_acceptance() {
        assert!(matches!(
            validate_chat_session_update(
                None,
                session(1),
                Uuid::new_v4(),
                Some(&FixedValidator(true)),
            ),
            ChatSessionUpdateOutcome::Accepted(_)
        ));
        assert!(matches!(
            validate_chat_session_update(
                None,
                session(1),
                Uuid::new_v4(),
                Some(&FixedValidator(false)),
            ),
            ChatSessionUpdateOutcome::Invalid(profile_key::ValidationError::InvalidSignature)
        ));
    }
}
