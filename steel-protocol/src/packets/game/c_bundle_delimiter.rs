//! Clientbound bundle delimiter packet - marks the start/end of a packet bundle.
//!
//! Packets sent between two bundle delimiters are processed atomically by the client
//! in a single game tick. This is used for entity spawning to ensure all related
//! packets (spawn, metadata, equipment, etc.) are applied together.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_BUNDLE_DELIMITER;

/// Marks the start or end of a packet bundle.
///
/// When the client receives this packet, it toggles bundle mode:
/// - First delimiter: Start collecting packets
/// - Second delimiter: Process all collected packets atomically
///
/// This packet has no fields - it's purely a marker.
#[derive(ClientPacket, WriteTo, Clone, Debug, Default)]
#[packet_id(Play = C_BUNDLE_DELIMITER)]
pub struct CBundleDelimiter;

impl CBundleDelimiter {
    /// Creates a new bundle delimiter packet.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}
