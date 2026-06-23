//! Clientbound set entity link packet.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SET_ENTITY_LINK;

/// Updates the leash/link holder for an entity.
#[derive(ClientPacket, WriteTo, Clone, Debug, PartialEq, Eq)]
#[packet_id(Play = C_SET_ENTITY_LINK)]
pub struct CSetEntityLink {
    /// Entity being leashed or cleared.
    pub source_id: i32,
    /// Leash holder entity id, or 0 to clear the link.
    pub dest_id: i32,
}

impl CSetEntityLink {
    /// Creates a new entity-link packet.
    #[must_use]
    pub const fn new(source_id: i32, dest_id: i32) -> Self {
        Self { source_id, dest_id }
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::serial::WriteTo as _;

    use super::*;

    #[test]
    fn entity_link_packet_uses_vanilla_raw_ints() {
        let packet = CSetEntityLink::new(42, 7);
        let mut bytes = Vec::new();

        packet.write(&mut bytes).expect("packet should encode");

        assert_eq!(bytes, vec![0, 0, 0, 42, 0, 0, 0, 7]);
    }
}
