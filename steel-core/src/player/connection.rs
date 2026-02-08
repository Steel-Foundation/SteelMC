//! This module contains the `PlayerConnection` trait that abstracts network connections.
//!
//! The trait is object-safe to allow using `dyn PlayerConnection` for both real network
//! connections (`JavaConnection`) and test connections (`FlintConnection`).

use enum_dispatch::enum_dispatch;
use steel_protocol::packet_traits::{CompressionInfo, EncodedPacket};
use text_components::TextComponent;

/// An object-safe trait for player connections.
///
/// This abstracts the connection layer so that:
/// - `JavaConnection` can handle real network traffic
/// - Test connections (like `FlintConnection`) can record events for assertions
///
/// # Object Safety
///
/// This trait uses type erasure for packet sending - packets must be pre-encoded
/// into `EncodedPacket` before being sent. The `Player` struct provides a generic
/// `send_packet<P: ClientPacket>()` helper that handles encoding.
#[enum_dispatch]
pub trait NetworkConnection: Send + Sync {
    /// Returns compression info for packet encoding.
    ///
    /// Returns `None` if compression is disabled (e.g., for test connections).
    fn compression(&self) -> Option<CompressionInfo>;

    /// Sends a pre-encoded packet.
    ///
    /// This is the object-safe method that accepts already-encoded packets.
    /// Use `Player::send_packet()` for the generic version that handles encoding.
    fn send_encoded(&self, packet: EncodedPacket);

    /// Sends multiple pre-encoded packets as an atomic bundle.
    ///
    /// The implementation wraps the packets with bundle delimiter packets so
    /// the client processes them together in a single game tick.
    /// Use `Player::send_bundle()` for the generic version that handles encoding.
    fn send_encoded_bundle(&self, packets: Vec<EncodedPacket>);

    /// Disconnects the player with a reason.
    fn disconnect_with_reason(&self, reason: TextComponent);

    /// Performs per-tick connection maintenance (e.g., keep-alive).
    fn tick(&self);

    /// Returns the current latency in milliseconds.
    fn latency(&self) -> i32;

    /// Closes the connection.
    fn close(&self);

    /// Returns whether the connection is closed.
    fn closed(&self) -> bool;
}

impl NetworkConnection for Box<dyn NetworkConnection> {
    fn compression(&self) -> Option<CompressionInfo> {
        (**self).compression()
    }

    fn send_encoded(&self, packet: EncodedPacket) {
        (**self).send_encoded(packet);
    }

    fn send_encoded_bundle(&self, packets: Vec<EncodedPacket>) {
        (**self).send_encoded_bundle(packets);
    }

    fn disconnect_with_reason(&self, reason: TextComponent) {
        (**self).disconnect_with_reason(reason);
    }

    fn tick(&self) {
        (**self).tick();
    }

    fn latency(&self) -> i32 {
        (**self).latency()
    }

    fn close(&self) {
        (**self).close();
    }

    fn closed(&self) -> bool {
        (**self).closed()
    }
}
