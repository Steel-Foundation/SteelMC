//! Server-side validation of client message acknowledgements.

use std::collections::VecDeque;

/// Maximum number of tracked messages for acknowledgement validation (Vanilla: 20)
const MAX_TRACKED_MESSAGES: usize = 20;

/// Entry tracking a sent message signature
#[derive(Debug, Clone)]
struct TrackedEntry {
    signature: Option<Box<[u8]>>,
    pending: bool, // true if not yet acknowledged by client
}

impl TrackedEntry {
    fn acknowledge(self) -> Self {
        Self {
            signature: self.signature,
            pending: false,
        }
    }
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
        let mut tracked_messages = VecDeque::with_capacity(MAX_TRACKED_MESSAGES);
        for _ in 0..MAX_TRACKED_MESSAGES {
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
    pub fn tracked_count(&self) -> usize {
        self.tracked_messages.len()
    }

    /// Applies an offset (removes old acknowledged messages from tracking)
    pub fn apply_offset(&mut self, offset: i32) -> Result<(), String> {
        let removable = self
            .tracked_messages
            .len()
            .saturating_sub(MAX_TRACKED_MESSAGES);
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
        acknowledged: [u8; 3], // 3 bytes = 24 bits, using 20
        offset: i32,
        checksum: u8,
    ) -> Result<Vec<Box<[u8]>>, String> {
        log::debug!(
            "apply_update: offset={}, checksum={}, tracked_messages.len()={}, acknowledged={:?}",
            offset,
            checksum,
            self.tracked_messages.len(),
            acknowledged
        );

        // First apply the offset to remove old messages
        self.apply_offset(offset)?;

        let mut acknowledged_signatures = Vec::new();

        // Process acknowledgements for the tracked window
        for i in 0..MAX_TRACKED_MESSAGES.min(self.tracked_messages.len()) {
            let bit_index = i;
            let byte_index = bit_index / 8;
            let bit_offset = bit_index % 8;
            let is_acknowledged = (acknowledged[byte_index] & (1 << bit_offset)) != 0;

            if let Some(entry_opt) = self.tracked_messages.get_mut(i) {
                if is_acknowledged {
                    // Client acknowledged this message
                    if let Some(entry) = entry_opt {
                        log::debug!(
                            "Index {}: Client acknowledged message (pending={})",
                            i,
                            entry.pending
                        );
                        let acknowledged_entry = entry.clone().acknowledge();
                        if let Some(sig) = &entry.signature {
                            acknowledged_signatures.push(sig.clone());
                        }
                        *entry_opt = Some(acknowledged_entry);
                    } else {
                        log::error!(
                            "Index {i}: Client acknowledged unknown/ignored message! tracked_messages[{i}] = None"
                        );
                        return Err(format!(
                            "Last seen update acknowledged unknown or previously ignored message at index {i}"
                        ));
                    }
                } else {
                    // Client did not acknowledge this message
                    if let Some(entry) = entry_opt
                        && !entry.pending
                    {
                        log::error!("Index {i}: Client ignored previously acknowledged message!");
                        return Err(format!(
                            "Last seen update ignored previously acknowledged message at index {i}"
                        ));
                    }
                    log::debug!("Index {i}: Client did not acknowledge (setting to None)");
                    // Set to None if not acknowledged
                    *entry_opt = None;
                }
            }
        }

        log::debug!(
            "apply_update: Successfully acknowledged {} signatures",
            acknowledged_signatures.len()
        );

        // TODO: Verify checksum if needed (checksum == 0 means skip validation)
        if checksum != 0 {
            // For now, we skip checksum validation
            // In a full implementation, compute checksum of acknowledged_signatures and compare
        }

        Ok(acknowledged_signatures)
    }
}
