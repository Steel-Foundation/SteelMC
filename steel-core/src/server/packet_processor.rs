//! Vanilla-style serverbound packet scheduling.

use std::sync::Arc;

use crossbeam::queue::SegQueue;

use crate::player::{Player, connection::NetworkConnection, networking::ScheduledPlayPacket};

use super::Server;

struct PendingPlayPacket {
    player: Arc<Player>,
    packet: ScheduledPlayPacket,
}

/// Packets submitted by network tasks and handled by the game scheduler before each tick.
pub(super) struct PacketProcessor {
    queued: PacketQueue<PendingPlayPacket>,
}

impl PacketProcessor {
    pub(super) const fn new() -> Self {
        Self {
            queued: PacketQueue::new(),
        }
    }

    pub(super) fn schedule(&self, player: Arc<Player>, packet: ScheduledPlayPacket) {
        self.queued.submit(PendingPlayPacket { player, packet });
    }

    pub(super) fn process_queued(&self, server: &Arc<Server>) {
        self.queued.process_all(|pending| {
            if pending.player.connection.closed()
                || server.cancel_token.is_cancelled()
                || pending.player.is_domain_switching()
            {
                return;
            }

            pending.packet.handle(pending.player, server);
        });
    }

    pub(super) fn clear(&self) {
        self.queued.clear();
    }
}

impl Default for PacketProcessor {
    fn default() -> Self {
        Self::new()
    }
}

/// Unbounded multi-producer FIFO matching Vanilla's packet processor queue.
struct PacketQueue<T> {
    queued: SegQueue<T>,
}

impl<T> PacketQueue<T> {
    const fn new() -> Self {
        Self {
            queued: SegQueue::new(),
        }
    }

    fn submit(&self, value: T) {
        self.queued.push(value);
    }

    /// Processes until the queue is observed empty, including submissions made during handling.
    fn process_all(&self, mut process: impl FnMut(T)) {
        while let Some(value) = self.queued.pop() {
            process(value);
        }
    }

    fn clear(&self) {
        while self.queued.pop().is_some() {}
    }
}

#[cfg(test)]
mod tests {
    use super::PacketQueue;

    #[test]
    fn queued_packets_are_processed_in_submission_order() {
        let queue = PacketQueue::new();
        queue.submit(1);
        queue.submit(2);
        queue.submit(3);

        let mut processed = Vec::new();
        queue.process_all(|value| processed.push(value));

        assert_eq!(processed, [1, 2, 3]);
    }

    #[test]
    fn processing_includes_packets_submitted_during_the_same_drain() {
        let queue = PacketQueue::new();
        queue.submit(1);
        let mut processed = Vec::new();

        queue.process_all(|value| {
            processed.push(value);
            if value == 1 {
                queue.submit(2);
            }
        });

        assert_eq!(processed, [1, 2]);
    }
}
