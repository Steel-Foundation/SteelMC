//! Chat message signature tracking for secure chat validation.

use std::collections::VecDeque;

use super::PreviousMessageEntry;

/// Maximum number of cached signatures (Vanilla: 128)
const MAX_CACHED_SIGNATURES: usize = 128;

/// Maximum number of previous messages to track (Vanilla: 20)
const MAX_PREVIOUS_MESSAGES: usize = 20;

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
        acknowledged: &[u8; 3], // FixedBitSet(20) = 3 bytes
    ) -> Option<LastSeen> {
        // Parse the 20-bit BitSet from 3 bytes
        let mut bits = [false; MAX_PREVIOUS_MESSAGES];
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
        if self.last_seen.0.len() >= MAX_PREVIOUS_MESSAGES {
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
    ///
    /// # Panics
    /// Panics if the deque is empty while attempting to pop (should never happen as we check `!deque.is_empty()`).
    pub fn push(&mut self, last_seen_signatures: &LastSeen, current_signature: Option<&[u8; 256]>) {
        use rustc_hash::FxHashSet;
        use std::collections::VecDeque;

        log::debug!(
            "push: adding {} lastSeen + {} current = {} total signatures to cache (current cache size: {})",
            last_seen_signatures.len(),
            i32::from(current_signature.is_some()),
            last_seen_signatures.len() + usize::from(current_signature.is_some()),
            self.full_cache.len()
        );

        // Build a deque with all signatures to push: lastSeen + current
        // Vanilla: addAll(list) then add(signature)
        let mut deque: VecDeque<Box<[u8]>> = VecDeque::new();

        // Add all lastSeen signatures (in order)
        for sig in last_seen_signatures.as_slice() {
            deque.push_back(sig.clone());
        }

        // Add current signature if present
        if let Some(sig) = current_signature {
            deque.push_back(Box::new(*sig));
        }

        // Create a set of all signatures we're pushing for O(1) lookup
        let push_set: FxHashSet<Box<[u8]>> = deque.iter().cloned().collect();

        // Vanilla's push algorithm:
        // for(int i = 0; !deque.isEmpty() && i < this.entries.length; ++i) {
        //     MessageSignature old = this.entries[i];
        //     this.entries[i] = deque.removeLast();  // Take from end (most recent)
        //     if (old != null && !set.contains(old)) {
        //         deque.addFirst(old);  // Re-add to front if not in push set
        //     }
        // }

        // Convert VecDeque to fixed-size array-like structure
        // We need to ensure cache has exactly 128 slots
        let mut new_cache = VecDeque::with_capacity(MAX_CACHED_SIGNATURES);

        let mut i = 0;
        while !deque.is_empty() && i < MAX_CACHED_SIGNATURES {
            // Get old entry at position i
            let old_entry = self.full_cache.get(i).cloned();

            // Take most recent from deque (from back)
            let new_entry = deque
                .pop_back()
                .expect("deque should not be empty due to loop condition");
            new_cache.push_back(new_entry);

            // If old entry exists and is not in our push set, add it back to process
            if let Some(old) = old_entry
                && !push_set.contains(&old)
            {
                deque.push_front(old);
            }

            i += 1;
        }

        self.full_cache = new_cache;
        log::debug!("push: cache updated, new size: {}", self.full_cache.len());
    }

    /// Convert the sender's `last_seen` signatures to IDs if the recipient has them in their cache.
    /// Otherwise, the full signature is sent. (ID:0 indicates full signature is being sent)
    #[must_use]
    pub fn index_previous_messages(
        &self,
        sender_last_seen: &LastSeen,
    ) -> Box<[PreviousMessageEntry]> {
        let mut indexed = Vec::new();

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
                indexed.push(PreviousMessageEntry {
                    // Send ID reference to recipient's cache (index + 1 because 0 is reserved for full signature)
                    id: 1 + index as i32,
                    signature: None,
                });
            } else {
                log::debug!("  lastSeen[{i}]: NOT in cache -> sending full signature (ID=0)");
                indexed.push(PreviousMessageEntry {
                    // Send ID as 0 for full signature
                    id: 0,
                    signature: Some(signature.clone()),
                });
            }
        }
        indexed.into_boxed_slice()
    }
}
