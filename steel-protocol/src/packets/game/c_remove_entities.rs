//! Packet sent to remove entities from the client.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_REMOVE_ENTITIES;

/// Removes one or more entities from the client.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_REMOVE_ENTITIES)]
pub struct CRemoveEntities {
    /// The entity IDs to remove
    #[write(as = Prefixed(VarInt, inner = VarInt))]
    pub entity_ids: Vec<i32>,
}

impl CRemoveEntities {
    /// Creates a packet to remove a single entity.
    #[must_use]
    pub fn single(entity_id: i32) -> Self {
        Self {
            entity_ids: vec![entity_id],
        }
    }
}
