//! Teleport state tracking for server-side position confirmation.
//!
//! When the server teleports a player, it assigns a teleport ID and waits for the
//! client to acknowledge it via `SAcceptTeleportation`. Until acknowledged, movement
//! packets are rejected. This struct manages that handshake.
//!
//! Vanilla: `ServerGamePacketListenerImpl.awaitingPositionFromClient`,
//! `awaitingTeleport`, `awaitingTeleportTime`.

use steel_utils::math::Vector3;

/// Tracks the state of a pending server-initiated teleport.
pub struct TeleportState {
    /// Position we're waiting for the client to confirm.
    /// `Some` means we should reject movement packets until confirmed.
    pub awaiting_position: Option<Vector3<f64>>,
    /// Incrementing teleport ID counter (wraps at `i32::MAX`).
    pub teleport_id: i32,
    /// Tick count when last teleport was sent (for timeout/resend).
    pub teleport_time: i32,
}

impl TeleportState {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            awaiting_position: None,
            teleport_id: 0,
            teleport_time: 0,
        }
    }

    /// Returns true if we're waiting for a teleport confirmation.
    #[must_use]
    pub const fn is_awaiting(&self) -> bool {
        self.awaiting_position.is_some()
    }

    /// Advances the teleport ID, wrapping at `i32::MAX`. Returns the new ID.
    pub const fn next_id(&mut self) -> i32 {
        self.teleport_id = if self.teleport_id == i32::MAX {
            0
        } else {
            self.teleport_id + 1
        };
        self.teleport_id
    }

    /// Accepts the teleport if the ID matches.
    /// Returns the confirmed position, or `None` if the ID doesn't match.
    pub const fn try_accept(&mut self, id: i32) -> Option<Vector3<f64>> {
        if id == self.teleport_id {
            self.awaiting_position.take()
        } else {
            None
        }
    }
}
