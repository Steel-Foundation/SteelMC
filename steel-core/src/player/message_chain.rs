//! Message chain management for signed chat.
//!
//! Ported from net/minecraft/network/chat/SignedMessageLink.java,
//! SignedMessageBody.java, and SignedMessageChain.java

use std::time::{SystemTime, UNIX_EPOCH};

use steel_crypto::{CryptError, SignatureUpdater, signature::SignatureOutput};
use thiserror::Error;
use uuid::Uuid;

use super::signature_cache::LastSeen;

/// A link in the signed message chain.
///
/// Equivalent to `SignedMessageLink` in Minecraft.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedMessageLink {
    /// Message index in the sequence (starts at 0)
    pub index: i32,
    /// UUID of the player sending the message
    pub sender: Uuid,
    /// UUID of the player's chat session
    pub session_id: Uuid,
}

impl SignedMessageLink {
    /// Creates a new message link
    #[must_use]
    pub const fn new(index: i32, sender: Uuid, session_id: Uuid) -> Self {
        Self {
            index,
            sender,
            session_id,
        }
    }

    /// Creates an unsigned message link (`session_id` = nil UUID)
    #[must_use]
    pub const fn unsigned(sender: Uuid) -> Self {
        Self::root(sender, Uuid::from_u128(0))
    }

    /// Creates the root (first) link in a chain
    #[must_use]
    pub const fn root(sender: Uuid, session_id: Uuid) -> Self {
        Self {
            index: 0,
            sender,
            session_id,
        }
    }

    /// Updates signature data with this link's information
    pub fn update_signature(&self, output: &mut dyn SignatureOutput) -> Result<(), CryptError> {
        // Sender UUID (16 bytes, as u128 big-endian)
        output.update(&self.sender.as_u128().to_be_bytes())?;

        // Session ID (16 bytes, as u128 big-endian)
        output.update(&self.session_id.as_u128().to_be_bytes())?;

        // Message index (4 bytes, i32 big-endian)
        output.update(&self.index.to_be_bytes())?;

        Ok(())
    }

    /// Checks if this link is a descendant of another (same sender/session, higher index)
    #[must_use]
    pub fn is_descendant_of(&self, other: &SignedMessageLink) -> bool {
        self.index > other.index
            && self.sender == other.sender
            && self.session_id == other.session_id
    }

    /// Advances to the next link in the chain (returns `None` if at max)
    #[must_use]
    pub const fn advance(&self) -> Option<Self> {
        if self.index == i32::MAX {
            None
        } else {
            Some(Self {
                index: self.index + 1,
                sender: self.sender,
                session_id: self.session_id,
            })
        }
    }
}

/// The body of a signed message containing content and metadata.
///
/// Equivalent to `SignedMessageBody` in Minecraft.
#[derive(Clone, Debug)]
pub struct SignedMessageBody {
    /// The message content (max 256 UTF-8 chars)
    pub content: String,
    /// When the message was created
    pub time_stamp: SystemTime,
    /// Random salt for uniqueness
    pub salt: i64,
    /// Previously seen message signatures
    pub last_seen: LastSeen,
}

impl SignedMessageBody {
    /// Creates a new signed message body
    #[must_use]
    pub const fn new(
        content: String,
        time_stamp: SystemTime,
        salt: i64,
        last_seen: LastSeen,
    ) -> Self {
        Self {
            content,
            time_stamp,
            salt,
            last_seen,
        }
    }

    /// Creates an unsigned message body (salt = 0, no last seen)
    #[must_use]
    pub fn unsigned(content: String) -> Self {
        Self {
            content,
            time_stamp: SystemTime::now(),
            salt: 0,
            last_seen: LastSeen::default(),
        }
    }

    /// Updates signature data with this body's information
    pub fn update_signature(&self, output: &mut dyn SignatureOutput) -> Result<(), CryptError> {
        // Salt (8 bytes, i64 big-endian)
        output.update(&self.salt.to_be_bytes())?;

        // Timestamp as epoch seconds (8 bytes, i64 big-endian)
        let epoch_seconds = self
            .time_stamp
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        output.update(&epoch_seconds.to_be_bytes())?;

        // Content length (4 bytes, i32 big-endian)
        let content_bytes = self.content.as_bytes();
        output.update(&(content_bytes.len() as i32).to_be_bytes())?;

        // Content (UTF-8 bytes)
        output.update(content_bytes)?;

        // Last seen signatures
        update_last_seen_signature(&self.last_seen, output)?;

        Ok(())
    }
}

/// Helper to update signature with last seen messages
fn update_last_seen_signature(
    last_seen: &LastSeen,
    output: &mut dyn SignatureOutput,
) -> Result<(), CryptError> {
    // Number of signatures (4 bytes, i32 big-endian)
    output.update(&(last_seen.len() as i32).to_be_bytes())?;

    // All signature bytes
    for signature in last_seen.as_slice() {
        output.update(signature)?;
    }

    Ok(())
}

/// Errors that can occur during message chain operations
#[derive(Debug, Error)]
pub enum ChainError {
    /// Profile key is missing
    #[error("Missing profile key")]
    MissingProfileKey,

    /// Chain is broken
    #[error("Chain is broken")]
    ChainBroken,

    /// Profile key has expired
    #[error("Profile key has expired")]
    ExpiredProfileKey,

    /// Invalid signature
    #[error("Invalid signature")]
    InvalidSignature,

    /// Message out of order
    #[error("Message out of order")]
    OutOfOrderChat,

    /// Message has expired
    #[error("Message has expired")]
    MessageExpired,

    /// Cryptographic error
    #[error("Cryptographic error: {0}")]
    CryptoError(#[from] CryptError),
}

/// Manages the message chain state for a player.
///
/// Equivalent to `SignedMessageChain` in Minecraft.
#[derive(Debug)]
pub struct SignedMessageChain {
    /// The next expected link in the chain (None if chain is broken)
    next_link: Option<SignedMessageLink>,
    /// Timestamp of the last message (for ordering validation)
    last_timestamp: SystemTime,
}

impl SignedMessageChain {
    /// Creates a new message chain
    #[must_use]
    pub const fn new(sender: Uuid, session_id: Uuid) -> Self {
        Self {
            next_link: Some(SignedMessageLink::root(sender, session_id)),
            last_timestamp: UNIX_EPOCH,
        }
    }

    /// Gets the current link if the chain is not broken
    #[must_use]
    pub const fn next_link(&self) -> Option<&SignedMessageLink> {
        self.next_link.as_ref()
    }

    /// Checks if the chain is broken
    #[must_use]
    pub const fn is_broken(&self) -> bool {
        self.next_link.is_none()
    }

    /// Breaks the chain (sets `next_link` to None)
    pub const fn break_chain(&mut self) {
        self.next_link = None;
    }

    /// Validates and advances the chain with a new message.
    ///
    /// Returns the link that was used for this message.
    ///
    /// # Errors
    /// Returns `ChainError` if validation fails
    pub fn validate_and_advance(
        &mut self,
        body: &SignedMessageBody,
    ) -> Result<SignedMessageLink, ChainError> {
        // Check chain not broken
        let link = self.next_link.clone().ok_or(ChainError::ChainBroken)?;

        // Check timestamp ordering (must be >= last)
        if body.time_stamp < self.last_timestamp {
            self.break_chain();
            return Err(ChainError::OutOfOrderChat);
        }

        // Update state
        self.last_timestamp = body.time_stamp;
        self.next_link = link.advance();

        Ok(link)
    }

    /// Resets the chain to a new session
    pub const fn reset(&mut self, sender: Uuid, session_id: Uuid) {
        self.next_link = Some(SignedMessageLink::root(sender, session_id));
        self.last_timestamp = UNIX_EPOCH;
    }
}

/// Helper struct to update signature with complete message data
pub struct MessageSignatureUpdater<'a> {
    link: &'a SignedMessageLink,
    body: &'a SignedMessageBody,
}

impl<'a> MessageSignatureUpdater<'a> {
    /// Creates a new message signature updater
    #[must_use]
    pub const fn new(link: &'a SignedMessageLink, body: &'a SignedMessageBody) -> Self {
        Self { link, body }
    }
}

impl SignatureUpdater for MessageSignatureUpdater<'_> {
    fn update(&self, output: &mut dyn SignatureOutput) -> Result<(), CryptError> {
        // Version number (always 1 as a 4-byte int, 00 00 00 01)
        output.update(&1i32.to_be_bytes())?;

        // Link data
        self.link.update_signature(output)?;

        // Body data
        self.body.update_signature(output)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_link_advance() {
        let link = SignedMessageLink::root(Uuid::new_v4(), Uuid::new_v4());
        assert_eq!(link.index, 0);

        let next = link.advance().expect("Should advance from index 0");
        assert_eq!(next.index, 1);
        assert_eq!(next.sender, link.sender);
        assert_eq!(next.session_id, link.session_id);
    }

    #[test]
    fn test_link_advance_at_max() {
        let link = SignedMessageLink::new(i32::MAX, Uuid::new_v4(), Uuid::new_v4());
        assert!(link.advance().is_none());
    }

    #[test]
    fn test_link_descendant() {
        let link1 = SignedMessageLink::root(Uuid::new_v4(), Uuid::new_v4());
        let link2 = link1.advance().expect("Should advance from root");
        let link3 = link2.advance().expect("Should advance from index 1");

        assert!(link2.is_descendant_of(&link1));
        assert!(link3.is_descendant_of(&link2));
        assert!(link3.is_descendant_of(&link1));
        assert!(!link1.is_descendant_of(&link2));
    }

    #[test]
    fn test_chain_validation() {
        let sender = Uuid::new_v4();
        let session = Uuid::new_v4();
        let mut chain = SignedMessageChain::new(sender, session);

        let body = SignedMessageBody::unsigned("Hello".to_string());
        let link = chain
            .validate_and_advance(&body)
            .expect("First message should validate");
        assert_eq!(link.index, 0);

        let body2 = SignedMessageBody::unsigned("World".to_string());
        let link2 = chain
            .validate_and_advance(&body2)
            .expect("Second message should validate");
        assert_eq!(link2.index, 1);
    }

    #[test]
    fn test_chain_out_of_order() {
        let sender = Uuid::new_v4();
        let session = Uuid::new_v4();
        let mut chain = SignedMessageChain::new(sender, session);

        let body1 = SignedMessageBody::unsigned("First".to_string());
        chain
            .validate_and_advance(&body1)
            .expect("First message should validate");

        // Create a message with an older timestamp
        let body2 =
            SignedMessageBody::new("Second".to_string(), UNIX_EPOCH, 0, LastSeen::default());

        let result = chain.validate_and_advance(&body2);
        assert!(matches!(result, Err(ChainError::OutOfOrderChat)));
        assert!(chain.is_broken());
    }
}
