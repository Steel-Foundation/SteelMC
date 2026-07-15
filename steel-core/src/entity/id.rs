//! Newtype for entity network IDs.

use std::fmt;

/// A unique entity network ID.
///
/// Allocated by [`next_entity_id`](super::next_entity_id) and sent over the wire
/// as a `VarInt`. Wraps the raw `i32` used by the protocol; convert to/from `i32`
/// at the packet boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(i32);

impl EntityId {
    /// Wraps a raw network id.
    #[must_use]
    pub const fn new(id: i32) -> Self {
        Self(id)
    }

    /// Returns the raw network id.
    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }
}

impl From<i32> for EntityId {
    fn from(id: i32) -> Self {
        Self(id)
    }
}

impl From<EntityId> for i32 {
    fn from(id: EntityId) -> Self {
        id.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
