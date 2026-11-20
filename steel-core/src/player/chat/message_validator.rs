//! Server-side validation of client message acknowledgements.

use std::collections::VecDeque;

use steel_protocol::packets::game::LastSeenMessagesUpdate;

const CHECKSUM_HASH_MULTIPLIER: u8 = 31;
const NO_CHECKSUM: u8 = 0;

/// Entry tracking a sent message signature
#[derive(Debug, Clone)]
struct TrackedEntry {
    signature: Option<Box<[u8]>>,
    pending: bool, // true if not yet acknowledged by client
}

/// Validates that the client is properly acknowledging messages sent by the server
#[derive(Debug)]
pub struct LastSeenMessagesValidator {
    tracked_messages: VecDeque<Option<TrackedEntry>>,
    last_pending_signature: Option<Box<[u8]>>,
}

impl Default for LastSeenMessagesValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl LastSeenMessagesValidator {
    /// Creates a new validator
    #[must_use]
    pub fn new() -> Self {
        let mut tracked_messages = VecDeque::with_capacity(LastSeenMessagesUpdate::MAX_ENTRIES);
        for _ in 0..LastSeenMessagesUpdate::MAX_ENTRIES {
            tracked_messages.push_back(None);
        }
        Self {
            tracked_messages,
            last_pending_signature: None,
        }
    }

    /// Adds a pending message signature that the client should acknowledge
    /// Matches vanilla's deduplication: only adds if different from last pending message
    pub fn add_pending(&mut self, signature: Option<Box<[u8]>>) {
        // Only add if this signature is different from the last one we added
        // This prevents duplicates when the same message is processed multiple times
        if signature.as_ref() != self.last_pending_signature.as_ref() {
            let entry = TrackedEntry {
                signature: signature.clone(),
                pending: true,
            };
            self.tracked_messages.push_back(Some(entry));
            self.last_pending_signature = signature;
        }
    }

    /// Gets the number of tracked messages
    #[must_use]
    pub const fn tracked_count(&self) -> usize {
        self.tracked_messages.len()
    }

    /// Applies an offset (removes old acknowledged messages from tracking)
    pub fn apply_offset(&mut self, offset: i32) -> Result<(), String> {
        let removable = self
            .tracked_messages
            .len()
            .saturating_sub(LastSeenMessagesUpdate::MAX_ENTRIES);
        if offset < 0 || offset as usize > removable {
            return Err(format!(
                "Advanced last seen window by {offset} messages, but expected at most {removable}"
            ));
        }
        for _ in 0..offset {
            self.tracked_messages.pop_front();
        }
        Ok(())
    }

    /// Applies an acknowledgement update from the client
    /// acknowledged: `BitSet` of 20 bits indicating which messages in the window are acknowledged
    /// offset: How many old messages to remove from the window
    /// checksum: Optional checksum for validation (0 = skip checksum)
    pub fn apply_update(
        &mut self,
        acknowledged: [u8; LastSeenMessagesUpdate::ACKNOWLEDGED_BYTES],
        offset: i32,
        checksum: u8,
    ) -> Result<Vec<Box<[u8]>>, String> {
        let max_entries = LastSeenMessagesUpdate::MAX_ENTRIES;
        let bits_per_byte = u8::BITS as usize;
        let used_bits_in_last_byte = max_entries % bits_per_byte;
        let last_byte = acknowledged[LastSeenMessagesUpdate::ACKNOWLEDGED_BYTES - 1];
        if used_bits_in_last_byte != 0 && last_byte >> used_bits_in_last_byte != 0 {
            return Err(format!(
                "Last seen update contained acknowledgements outside the {max_entries}-message window"
            ));
        }

        self.apply_offset(offset)?;

        let count = acknowledged
            .iter()
            .map(|byte| byte.count_ones())
            .sum::<u32>() as usize;
        let mut acknowledged_signatures = Vec::with_capacity(count);

        for (index, tracked) in self
            .tracked_messages
            .iter_mut()
            .take(max_entries)
            .enumerate()
        {
            let is_acknowledged =
                (acknowledged[index / bits_per_byte] & (1 << (index % bits_per_byte))) != 0;

            if is_acknowledged {
                let Some(entry) = tracked else {
                    return Err(format!(
                        "Last seen update acknowledged unknown or previously ignored message at index {index}"
                    ));
                };

                entry.pending = false;
                if let Some(signature) = &entry.signature {
                    acknowledged_signatures.push(signature.clone());
                }
            } else {
                if tracked.as_ref().is_some_and(|entry| !entry.pending) {
                    return Err(format!(
                        "Last seen update ignored previously acknowledged message at index {index}"
                    ));
                }

                *tracked = None;
            }
        }

        if checksum != NO_CHECKSUM {
            let computed = acknowledged_signatures.iter().fold(1_u8, |sum, signature| {
                let hash = signature.iter().fold(1_u8, |hash, byte| {
                    hash.wrapping_mul(CHECKSUM_HASH_MULTIPLIER)
                        .wrapping_add(*byte)
                });

                sum.wrapping_mul(CHECKSUM_HASH_MULTIPLIER)
                    .wrapping_add(hash)
            });

            let expected = if computed == NO_CHECKSUM { 1 } else { computed };
            if checksum != expected {
                return Err("Checksum mismatch on last seen update".to_owned());
            }
        }

        Ok(acknowledged_signatures)
    }
}
