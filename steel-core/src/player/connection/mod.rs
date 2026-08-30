//! This module contains the `PlayerConnection` trait that abstracts network connections.
//!
//! The trait is object-safe to allow using `dyn PlayerConnection` for both real network
//! connections (`JavaConnection`) and test connections (`FlintConnection`).

mod java;

pub use java::{BundleBuilder, JavaConnection, JavaNetworkWriter, OutboundPacket};
pub(crate) use java::{ScheduledPacketExecution, ScheduledPlayPacket};

use std::time::Duration;

use enum_dispatch::enum_dispatch;
use steel_protocol::packet_traits::{ClientPacket, CompressionInfo, EncodedPacket};
use steel_protocol::packet_writer::TCPNetworkEncoder;
use steel_protocol::packets::common::{
    ChatVisibility, HumanoidArm, ParticleStatus, SClientInformation,
};
use steel_protocol::packets::game::CPlayerInfoUpdate;
use steel_protocol::utils::{ConnectionProtocol, PacketError};
use steel_utils::locks::AsyncMutex;
use text_components::TextComponent;
use tokio::io::AsyncWrite;
use tokio::time::timeout;

use crate::player::Player;

/// Maximum number of outbound packets drained from the channel per write batch.
pub const OUTBOUND_BATCH_SIZE: usize = 128;

/// Maximum time an outbound batch write may stay pending on the socket before the
/// encoder is discarded and the connection is treated as failed.
///
/// A client that stops reading fills its socket buffer and leaves the write pending
/// indefinitely; without a deadline the sender task would hold the encoder and the
/// writer lock until the connection dies, wedging kicks, keepalive timeouts, and
/// shutdown. Healthy clients drain a batch in microseconds, so only a stalled peer
/// ever hits this; 30 s matches vanilla's disconnect-timeout scale.
pub const OUTBOUND_WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// Writes a drained batch of outbound packets under a single writer lock, keeping
/// bundles contiguous and stopping at a disconnect. Returns whether a disconnect was
/// written, in which case the connection should close afterwards.
///
/// The encoder is taken out of the shared writer slot and only restored on success:
/// a batch that fails or stalls partway leaves the CFB8 cipher state mid-packet, so
/// the encoder is dropped instead of being reused for later packets. The write is
/// bounded by [`OUTBOUND_WRITE_TIMEOUT`] so a client that stops reading cannot wedge
/// the sender task.
pub async fn write_outbound_batch<W: AsyncWrite + Unpin>(
    network_writer: &AsyncMutex<Option<TCPNetworkEncoder<W>>>,
    batch: &mut Vec<OutboundPacket>,
) -> Result<bool, PacketError> {
    write_outbound_batch_within(network_writer, batch, OUTBOUND_WRITE_TIMEOUT).await
}

/// [`write_outbound_batch`] with an explicit stall deadline; tests use a small one.
async fn write_outbound_batch_within<W: AsyncWrite + Unpin>(
    network_writer: &AsyncMutex<Option<TCPNetworkEncoder<W>>>,
    batch: &mut Vec<OutboundPacket>,
    write_timeout: Duration,
) -> Result<bool, PacketError> {
    let write = async {
        let mut writer_guard = network_writer.lock().await;
        let Some(mut writer) = writer_guard.take() else {
            return Err(PacketError::ConnectionClosed);
        };

        let mut close_after_write = false;
        for outbound in batch.drain(..) {
            match outbound {
                OutboundPacket::Packet(packet) => writer.write_packet_buffered(&packet).await?,
                OutboundPacket::Bundle(bundle) => {
                    for packet in bundle {
                        writer.write_packet_buffered(&packet).await?;
                    }
                }
                OutboundPacket::Disconnect(packet) => {
                    writer.write_packet_buffered(&packet).await?;
                    close_after_write = true;
                    break;
                }
            }
        }

        writer.flush().await?;
        *writer_guard = Some(writer);
        Ok(close_after_write)
    };

    // Timing out drops the in-flight write together with the taken encoder and the
    // lock guard, poisoning the writer slot by absence.
    match timeout(write_timeout, write).await {
        Ok(result) => result,
        Err(_) => Err(PacketError::WriteTimeout),
    }
}

/// Client-side settings sent via `SClientInformation` packet.
/// This is stored separately from the packet struct to allow default initialization.
#[derive(Debug, Clone)]
pub struct ClientInformation {
    /// The client's language (e.g., "`en_us"`).
    pub language: String,
    /// The client's requested view distance in chunks.
    pub view_distance: u8,
    /// Chat visibility setting.
    pub chat_visibility: ChatVisibility,
    /// Whether chat colors are enabled.
    pub chat_colors: bool,
    /// Bitmask for displayed skin parts.
    pub model_customization: u8,
    /// The player's main hand (left or right).
    pub main_hand: HumanoidArm,
    /// Whether text filtering is enabled.
    pub text_filtering_enabled: bool,
    /// Whether the player appears in the server list.
    pub allows_listing: bool,
    /// Particle rendering setting.
    pub particle_status: ParticleStatus,
}

impl Default for ClientInformation {
    fn default() -> Self {
        Self {
            language: "en_us".to_string(),
            view_distance: 8,
            chat_visibility: ChatVisibility::Full,
            chat_colors: true,
            model_customization: 0,
            main_hand: HumanoidArm::Right,
            text_filtering_enabled: false,
            allows_listing: true,
            particle_status: ParticleStatus::All,
        }
    }
}

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

/// Concrete player connection type using `enum_dispatch` for zero-cost dispatch.
///
/// The `Java` variant handles real Java connections while `Other` supports tests
/// and alternative backends.
#[enum_dispatch(NetworkConnection)]
pub enum PlayerConnection {
    /// A real Java client connection.
    Java(JavaConnection),
    /// A dynamic connection for tests or other backends.
    Other(Box<dyn NetworkConnection>),
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
    /// The closure receives a [`BundleBuilder`] to add packets to.
    /// All packets are encoded, then sent wrapped in bundle delimiters so the
    /// client processes them together in a single game tick.
    pub fn send_bundle<F>(&self, f: F)
    where
        F: FnOnce(&mut BundleBuilder),
    {
        let mut builder = BundleBuilder::new(self.connection.compression());
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

    /// Marks the player's connection as closed without sending a disconnect
    /// packet.
    ///
    /// Used during shutdown so that container-close logic treats the player as
    /// disconnected — dropping open-menu contents into the world instead of the
    /// saved inventory, matching vanilla's removal-on-shutdown behavior.
    pub fn close_connection(&self) {
        self.connection.close();
    }

    /// Handles client information updates during play phase.
    pub fn handle_client_information(&self, packet: SClientInformation) {
        let old_view_distance = self.view_distance();
        let was_hat_shown = self.shows_hat();

        let info = ClientInformation {
            language: packet.language,
            view_distance: packet
                .view_distance
                .max(2)
                .cast_unsigned()
                .min(self.config.view_distance.max(2)),
            chat_visibility: packet.chat_visibility,
            chat_colors: packet.chat_colors,
            model_customization: packet.model_customization,
            main_hand: packet.main_hand,
            text_filtering_enabled: packet.text_filtering_enabled,
            allows_listing: packet.allows_listing,
            particle_status: packet.particle_status,
        };
        self.set_client_information(info);

        let show_hat = self.shows_hat();
        if show_hat != was_hat_shown {
            self.server()
                .broadcast_to_online(CPlayerInfoUpdate::update_hat(self.gameprofile.id, show_hat));
        }

        // Vanilla does not echo CSetChunkCacheRadius here; it is only broadcast
        // when the server-wide view distance changes.
        if old_view_distance != self.view_distance() {
            self.get_world().chunk_map.update_player_status(self);
        }
    }

    /// Returns the player's client information settings.
    #[must_use]
    pub fn client_information(&self) -> ClientInformation {
        self.client_information.lock().clone()
    }

    /// Updates the player's client information settings.
    pub fn set_client_information(&self, info: ClientInformation) {
        Self::apply_client_information_to_entity_data(&mut self.entity_data.lock(), &info);
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

#[cfg(test)]
mod tests {
    use std::io;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use steel_protocol::packets::common::CKeepAlive;
    use tokio::io::AsyncWrite;

    use super::*;

    fn encoded_keep_alive(id: i64) -> EncodedPacket {
        EncodedPacket::from_bare(CKeepAlive::new(id), None, ConnectionProtocol::Play)
            .expect("keep alive should encode")
    }

    /// Writer that accepts a single poll, then fails: forces a batch to fail partway.
    struct FailAfterFirstPoll {
        first: bool,
    }

    impl AsyncWrite for FailAfterFirstPoll {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            let this = self.get_mut();
            if this.first {
                this.first = false;
                Poll::Ready(Ok(buf.len()))
            } else {
                Poll::Ready(Err(io::Error::other("write failed")))
            }
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn failed_batch_write_discards_the_encoder() {
        let network_writer = AsyncMutex::new(Some(TCPNetworkEncoder::new(FailAfterFirstPoll {
            first: true,
        })));
        network_writer
            .lock()
            .await
            .as_mut()
            .expect("encoder present")
            .set_encryption(&[0x42; 16]);

        let mut batch = vec![
            OutboundPacket::Packet(encoded_keep_alive(1)),
            OutboundPacket::Packet(encoded_keep_alive(2)),
        ];

        assert!(
            write_outbound_batch(&network_writer, &mut batch)
                .await
                .is_err()
        );
        // The encoder held mid-packet cipher state; it must be gone, not reused.
        assert!(network_writer.lock().await.is_none());
    }

    #[tokio::test]
    async fn successful_batch_write_restores_the_encoder() {
        let network_writer = AsyncMutex::new(Some(TCPNetworkEncoder::new(Vec::new())));

        let mut batch = vec![
            OutboundPacket::Packet(encoded_keep_alive(1)),
            OutboundPacket::Disconnect(encoded_keep_alive(2)),
        ];
        assert!(matches!(
            write_outbound_batch(&network_writer, &mut batch).await,
            Ok(true)
        ));
        assert!(network_writer.lock().await.is_some());
    }

    /// Writer that never accepts data: simulates a client that stopped reading.
    struct StalledWriter;

    impl AsyncWrite for StalledWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Pending
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Pending
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Pending
        }
    }

    #[tokio::test]
    async fn stalled_batch_write_times_out_and_discards_the_encoder() {
        let network_writer = AsyncMutex::new(Some(TCPNetworkEncoder::new(StalledWriter)));
        network_writer
            .lock()
            .await
            .as_mut()
            .expect("encoder present")
            .set_encryption(&[0x42; 16]);

        let mut batch = vec![OutboundPacket::Packet(encoded_keep_alive(1))];
        let result =
            write_outbound_batch_within(&network_writer, &mut batch, Duration::from_millis(10))
                .await;

        assert!(matches!(result, Err(PacketError::WriteTimeout)));
        // A stalled write leaves mid-packet cipher state; the encoder must be gone.
        assert!(network_writer.lock().await.is_none());
    }
}
