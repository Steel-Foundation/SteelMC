use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use steel_protocol::packet_traits::{CompressionInfo, EncodedPacket};
use steel_utils::locks::SyncMutex;
use text_components::TextComponent;

use crate::player::connection::NetworkConnection;

/// One event observed by a [`RecordingConnection`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordedConnectionEvent {
    /// One encoded client-bound packet.
    Packet(Vec<u8>),
    /// One atomic encoded packet bundle.
    Bundle(Vec<Vec<u8>>),
    /// A server-requested disconnect.
    Disconnected(String),
}

#[derive(Default)]
struct RecordingConnectionState {
    events: SyncMutex<Vec<RecordedConnectionEvent>>,
    closed: AtomicBool,
}

/// Cloneable observer for the non-networked connection attached to a test player.
#[derive(Clone, Default)]
pub struct RecordingConnection {
    state: Arc<RecordingConnectionState>,
}

impl RecordingConnection {
    /// Returns a stable snapshot of all connection events in send order.
    #[must_use]
    pub fn events(&self) -> Vec<RecordedConnectionEvent> {
        self.state.events.lock().clone()
    }

    /// Clears previously recorded connection events.
    pub fn clear(&self) {
        self.state.events.lock().clear();
    }

    /// Returns whether Steel closed this connection.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.state.closed.load(Ordering::Acquire)
    }
}

impl NetworkConnection for RecordingConnection {
    fn compression(&self) -> Option<CompressionInfo> {
        None
    }

    fn send_encoded(&self, packet: EncodedPacket) {
        if self.is_closed() {
            return;
        }
        self.state
            .events
            .lock()
            .push(RecordedConnectionEvent::Packet(
                packet.encoded_data.as_slice().to_vec(),
            ));
    }

    fn send_encoded_bundle(&self, packets: Vec<EncodedPacket>) {
        if self.is_closed() {
            return;
        }
        self.state
            .events
            .lock()
            .push(RecordedConnectionEvent::Bundle(
                packets
                    .into_iter()
                    .map(|packet| packet.encoded_data.as_slice().to_vec())
                    .collect(),
            ));
    }

    fn disconnect_with_reason(&self, reason: TextComponent) {
        self.state
            .events
            .lock()
            .push(RecordedConnectionEvent::Disconnected(format!("{reason:?}")));
        self.close();
    }

    fn tick(&self) {}

    fn latency(&self) -> i32 {
        0
    }

    fn close(&self) {
        self.state.closed.store(true, Ordering::Release);
    }

    fn closed(&self) -> bool {
        self.is_closed()
    }
}
