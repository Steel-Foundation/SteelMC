//! Chat and messaging state for a player.
//!
//! Groups the fields related to secure chat: message counters, signature cache,
//! message validator, chat session, and message chain.

use super::LastSeenMessagesValidator;
use super::MessageCache;
use super::message_chain::SignedMessageChain;
use super::profile_key::RemoteChatSession;

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
