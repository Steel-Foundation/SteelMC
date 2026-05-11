//! This module contains the `PlayerConnection` trait that abstracts network connections.
//!
//! The trait is object-safe to allow using `dyn PlayerConnection` for both real network
//! connections (`JavaConnection`) and test connections (`FlintConnection`).

use enum_dispatch::enum_dispatch;
use steel_protocol::packet_traits::{ClientPacket, CompressionInfo, EncodedPacket};
use steel_protocol::packets::common::SClientInformation;
use steel_protocol::packets::game::CSetChunkCacheRadius;
use steel_protocol::utils::ConnectionProtocol;
use text_components::TextComponent;

use crate::player::{ClientInformation, Player, networking};

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

impl Player {
    /// Sends a packet to the player's connection.
    ///
    /// This is a generic helper that encodes the packet and delegates to the
    /// connection's `send_encoded` method, enabling object-safe packet sending.
    ///
    /// # Panics
    ///
    /// Panics if the packet fails to encode.
    pub fn send_packet<P: ClientPacket>(&self, packet: P) {
        let encoded = EncodedPacket::from_bare(
            packet,
            self.connection.compression(),
            ConnectionProtocol::Play,
        )
        .expect("Failed to encode packet");
        self.connection.send_encoded(encoded);
    }

    /// Sends multiple packets as an atomic bundle.
    ///
    /// The closure receives a [`BundleBuilder`](networking::BundleBuilder) to add packets to.
    /// All packets are encoded, then sent wrapped in bundle delimiters so the
    /// client processes them together in a single game tick.
    pub fn send_bundle<F>(&self, f: F)
    where
        F: FnOnce(&mut networking::BundleBuilder),
    {
        let mut builder = networking::BundleBuilder::new(self.connection.compression());
        f(&mut builder);
        let packets = builder.into_packets();
        if !packets.is_empty() {
            self.connection.send_encoded_bundle(packets);
        }
    }

    /// Disconnects the player with a reason message.
    pub fn disconnect(&self, reason: impl Into<TextComponent>) {
        self.connection.disconnect_with_reason(reason.into());
    }

    /// Handles client information updates during play phase.
    pub fn handle_client_information(&self, packet: SClientInformation) {
        let old_view_distance = self.view_distance();

        let info = ClientInformation {
            language: packet.language,
            view_distance: packet.view_distance.clamp(2, 32) as u8,
            chat_visibility: packet.chat_visibility,
            chat_colors: packet.chat_colors,
            model_customization: packet.model_customization,
            main_hand: packet.main_hand,
            text_filtering_enabled: packet.text_filtering_enabled,
            allows_listing: packet.allows_listing,
            particle_status: packet.particle_status,
        };
        self.set_client_information(info);

        let new_view_distance = self.view_distance();
        if old_view_distance != new_view_distance {
            self.send_packet(CSetChunkCacheRadius {
                radius: i32::from(new_view_distance),
            });
        }
    }

    /// Returns the player's client information settings.
    #[must_use]
    pub fn client_information(&self) -> ClientInformation {
        self.client_information.lock().clone()
    }

    /// Updates the player's client information settings.
    pub fn set_client_information(&self, info: ClientInformation) {
        *self.client_information.lock() = info;
    }

    /// Returns the effective view distance for this player.
    ///
    /// This is the minimum of the client's requested view distance and
    /// the server's configured maximum view distance.
    #[must_use]
    pub fn view_distance(&self) -> u8 {
        let client_view_distance = self.client_information.lock().view_distance;
        client_view_distance.min(self.world.load().view_distance)
    }
}
