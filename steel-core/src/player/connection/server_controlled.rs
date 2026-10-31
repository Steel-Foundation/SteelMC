//! A server-owned player has no network peer. Entity updates still go through the
//! normal tracker to real observers; outbound packets to the controlled actor end here.
use super::NetworkConnection;
use std::sync::atomic::{AtomicBool, Ordering};
use steel_protocol::packet_traits::{CompressionInfo, EncodedPacket};
use text_components::TextComponent;
#[derive(Default)]
pub(crate) struct ServerControlledConnection {
    closed: AtomicBool,
}
impl NetworkConnection for ServerControlledConnection {
    fn is_server_controlled(&self) -> bool {
        true
    }
    fn compression(&self) -> Option<CompressionInfo> {
        None
    }
    fn send_encoded(&self, _: EncodedPacket) {}
    fn send_encoded_bundle(&self, _: Vec<EncodedPacket>) {}
    fn disconnect_with_reason(&self, _: TextComponent) {
        self.close();
    }
    fn tick(&self) {}
    fn latency(&self) -> i32 {
        0
    }
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }
    fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
}
