//! Test connection implementation for Flint tests.
//!
//! This module provides a mock connection that records events (packets sent, disconnects)
//! instead of sending them over the network.

use std::sync::atomic::{AtomicBool, Ordering};

use steel_core::player::connection::NetworkConnection;
use steel_protocol::packet_traits::{CompressionInfo, EncodedPacket};
use steel_utils::locks::SyncMutex;
use text_components::TextComponent;

/// An event that occurred on the connection, for test assertions.
#[derive(Debug, Clone)]
pub enum PlayerEvent {
    /// A packet was sent to the player.
    PacketSent {
        /// The raw encoded packet data.
        data: Vec<u8>,
    },
    /// The player was disconnected.
    Disconnected {
        /// The disconnect reason.
        reason: String,
    },
}

/// A mock connection for Flint tests.
///
/// This connection records events instead of sending packets over the network,
/// allowing tests to verify what packets would have been sent.
pub struct FlintConnection {
    /// Recorded events for test assertions.
    events: SyncMutex<Vec<PlayerEvent>>,
    /// Whether the connection is closed.
    closed: AtomicBool,
}

impl FlintConnection {
    /// Creates a new test connection.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            events: SyncMutex::new(Vec::new()),
            closed: AtomicBool::new(false),
        }
    }

    /// Gets all recorded events.
    pub fn get_events(&self) -> Vec<PlayerEvent> {
        self.events.lock().clone()
    }

    /// Clears all recorded events.
    pub fn clear_events(&self) {
        self.events.lock().clear();
    }

    /// Returns the number of recorded events.
    pub fn event_count(&self) -> usize {
        self.events.lock().len()
    }
}

impl Default for FlintConnection {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkConnection for FlintConnection {
    fn compression(&self) -> Option<CompressionInfo> {
        // No compression for tests - simpler packet handling
        None
    }

    fn send_encoded(&self, packet: EncodedPacket) {
        if !self.closed.load(Ordering::Relaxed) {
            self.events.lock().push(PlayerEvent::PacketSent {
                data: packet.encoded_data.as_slice().to_vec(),
            });
        }
    }

    fn send_encoded_bundle(&self, packets: Vec<EncodedPacket>) {
        if !self.closed.load(Ordering::Relaxed) {
            let mut events = self.events.lock();
            for packet in packets {
                events.push(PlayerEvent::PacketSent {
                    data: packet.encoded_data.as_slice().to_vec(),
                });
            }
        }
    }

    fn disconnect_with_reason(&self, reason: TextComponent) {
        self.events.lock().push(PlayerEvent::Disconnected {
            reason: format!("{reason:?}"),
        });
        self.closed.store(true, Ordering::Relaxed);
    }

    fn tick(&self) {
        // No-op for tests - no keep-alive needed
    }

    fn latency(&self) -> i32 {
        // Perfect connection for tests
        0
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
    }

    fn closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }
}
