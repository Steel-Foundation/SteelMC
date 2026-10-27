//! Mock connection for Flint tests: drops every packet instead of sending it.

use std::sync::atomic::{AtomicBool, Ordering};

use steel_core::player::connection::NetworkConnection;
use steel_protocol::packet_traits::{CompressionInfo, EncodedPacket};
use text_components::TextComponent;

/// Drops outgoing packets; only tracks whether the connection was closed.
#[derive(Default)]
pub struct FlintConnection {
    closed: AtomicBool,
}

impl NetworkConnection for FlintConnection {
    fn compression(&self) -> Option<CompressionInfo> {
        None
    }

    fn send_encoded(&self, _packet: EncodedPacket) {}

    fn send_encoded_bundle(&self, _packets: Vec<EncodedPacket>) {}

    fn disconnect_with_reason(&self, _reason: TextComponent) {
        self.close();
    }

    fn tick(&self) {}

    fn latency(&self) -> i32 {
        0
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
    }

    fn closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }
}
