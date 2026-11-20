//! Chat message signature tracking for secure chat validation.

use std::collections::VecDeque;
use std::mem;

use rustc_hash::FxHashSet;
use steel_protocol::packets::game::{LastSeenMessagesUpdate, MessageSignature, PreviousMessage};

/// Maximum number of cached signatures (Vanilla: 128)
const MAX_CACHED_SIGNATURES: usize = 128;

/// Tracks the last seen message signatures by a player
#[derive(Debug, Clone, Default)]
pub struct LastSeen(Vec<Box<[u8]>>);

impl LastSeen {
    /// Creates a new `LastSeen` from a vector of signatures
    #[must_use]
    pub const fn new(signatures: Vec<Box<[u8]>>) -> Self {
        Self(signatures)
    }

    /// Gets the underlying vector of signatures
    #[must_use]
    pub fn as_slice(&self) -> &[Box<[u8]>] {
        &self.0
    }

    /// Gets the number of tracked signatures
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Checks if there are no tracked signatures
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Message signature cache for a player
#[derive(Debug)]
pub struct MessageCache {
    /// Max 128 cached message signatures. Most recent FIRST.
    /// Server should (when possible) reference indexes in this (recipient's) cache
    /// instead of sending full signatures in last seen.
    /// Must be 1:1 with client's signature cache.
    full_cache: VecDeque<Box<[u8]>>,

    /// Max 20 last seen messages by the sender. Most Recent LAST
    pub last_seen: LastSeen,
}

impl Default for MessageCache {
    fn default() -> Self {
        Self {
            full_cache: VecDeque::with_capacity(MAX_CACHED_SIGNATURES),
            last_seen: LastSeen::default(),
        }
    }
}

impl MessageCache {
    /// Creates a new message cache
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reconstructs the `LastSeen` from a `BitSet` acknowledgment.
    ///
    /// The `offset` indicates how many old messages to skip (not used for unpacking,
    /// but used by the validator), and the `acknowledged` `BitSet` indicates
    /// which of the last 20 messages were seen.
    ///
    /// Returns None if the cache doesn't contain the required messages.
    ///
    /// Note: The offset is primarily used by the `LastSeenMessagesValidator` to advance
    /// the tracking window. For unpacking acknowledged messages, we just need to look
    /// at which bits are set in the acknowledged bitset and retrieve those signatures
    /// from the cache at the corresponding indices.
    #[must_use]
    pub fn unpack_acknowledged(
        &self,
        _offset: i32,
        acknowledged: [u8; LastSeenMessagesUpdate::ACKNOWLEDGED_BYTES],
    ) -> Option<LastSeen> {
        // Parse the 20-bit BitSet from 3 bytes
        let mut bits = [false; LastSeenMessagesUpdate::MAX_ENTRIES];

        for (i, bit) in bits.iter_mut().enumerate() {
            let byte_index = i / 8;
            let bit_index = i % 8;
            *bit = (acknowledged[byte_index] & (1 << bit_index)) != 0;
        }

        // Collect acknowledged signatures from the cache
        let mut signatures = Vec::new();

        // Iterate through the bits to find acknowledged messages
        // The cache is ordered with most recent messages first (index 0)
        // The acknowledged bitset maps directly to cache indices
        for (i, &is_acknowledged) in bits.iter().enumerate() {
            if !is_acknowledged {
                continue; // This message was not acknowledged
            }

            // The index in the acknowledged bitset corresponds to the cache index
            let cache_index = i;

            if cache_index >= self.full_cache.len() {
                // Cache doesn't have this message
                log::warn!(
                    "Cache miss: trying to access index {} but cache only has {} entries",
                    cache_index,
                    self.full_cache.len()
                );
                return None;
            }

            signatures.push(self.full_cache[cache_index].clone());
        }

        Some(LastSeen(signatures))
    }

    /// Cache signatures from senders that the recipient hasn't seen yet.
    /// Not used for caching seen messages. Only for non-indexed signatures from senders.
    pub fn cache_signatures(&mut self, signatures: &[Box<[u8]>]) {
        for sig in signatures.iter().rev() {
            if self.full_cache.contains(sig) {
                continue;
            }
            // If the cache is maxed, and someone sends a signature older than the oldest in cache, ignore it
            if self.full_cache.len() < MAX_CACHED_SIGNATURES {
                self.full_cache.push_back(sig.clone()); // Recipient never saw this message so it must be older than the oldest in cache
            }
        }
    }

    /// Adds a seen signature to `last_seen` and `full_cache`.
    pub fn add_seen_signature(&mut self, signature: &[u8]) {
        if self.last_seen.0.len() >= LastSeenMessagesUpdate::MAX_ENTRIES {
            self.last_seen.0.remove(0);
        }

        self.last_seen.0.push(signature.into());

        // This probably doesn't need to be a loop, but better safe than sorry
        while self.full_cache.len() >= MAX_CACHED_SIGNATURES {
            self.full_cache.pop_back();
        }
        self.full_cache.push_front(signature.into()); // Since recipient saw this message it will be most recent in cache
    }

    /// Pushes signatures into the cache using vanilla's algorithm.
    /// This should be called AFTER sending a chat packet to a recipient.
    ///
    /// The signatures are pushed in order: all lastSeen signatures first, then the current message signature.
    /// This matches vanilla's MessageSignatureCache.push(SignedMessageBody, `MessageSignature`) behavior.
    pub fn push(
        &mut self,
        last_seen_signatures: &LastSeen,
        current_signature: Option<&[u8; MessageSignature::BYTES]>,
    ) {
        let signatures = last_seen_signatures
            .as_slice()
            .iter()
            .map(Box::as_ref)
            .chain(current_signature.map(<[_; MessageSignature::BYTES]>::as_slice));
        let mut queue: VecDeque<Box<[u8]>> = signatures.clone().map(Box::from).collect();
        let added: FxHashSet<&[u8]> = signatures.collect();

        for index in 0..MAX_CACHED_SIGNATURES {
            let Some(signature) = queue.pop_back() else {
                break;
            };

            if let Some(entry) = self.full_cache.get_mut(index) {
                let previous = mem::replace(entry, signature);
                if !added.contains(previous.as_ref()) {
                    queue.push_front(previous);
                }
            } else {
                self.full_cache.push_back(signature);
            }
        }
    }

    /// Convert the sender's `last_seen` signatures to IDs if the recipient has them in their cache.
    /// Otherwise, the full signature is sent. (ID:0 indicates full signature is being sent)
    #[must_use]
    pub fn index_previous_messages(&self, sender_last_seen: &LastSeen) -> Box<[PreviousMessage]> {
        let mut indexed = Vec::with_capacity(sender_last_seen.len());

        log::debug!(
            "index_previous_messages: sender has {} lastSeen signatures, recipient cache size: {}",
            sender_last_seen.len(),
            self.full_cache.len()
        );

        for (i, signature) in sender_last_seen.as_slice().iter().enumerate() {
            let index = self.full_cache.iter().position(|s| s == signature);

            if let Some(index) = index {
                log::debug!(
                    "  lastSeen[{}]: found in cache at index {} -> sending ID={}",
                    i,
                    index,
                    index + 1
                );
                indexed.push(PreviousMessage {
                    // Send ID reference to recipient's cache (index + 1 because 0 is reserved for full signature)
                    id: PreviousMessage::FULL_SIGNATURE_ID + 1 + index as i32,
                    signature: None,
                });
            } else {
                log::debug!("  lastSeen[{i}]: NOT in cache -> sending full signature (ID=0)");
                indexed.push(PreviousMessage {
                    // Send ID as 0 for full signature
                    id: PreviousMessage::FULL_SIGNATURE_ID,
                    signature: Some(signature.clone()),
                });
            }
        }
        indexed.into_boxed_slice()
    }
}
