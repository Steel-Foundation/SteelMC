use glam::DVec3;
use steel_macros::ServerPacket;
use steel_utils::codec::{LpVec3, VarInt};
use steel_utils::serial::ReadFrom;
use steel_utils::types::InteractionHand;

/// Serverbound packet sent when a player right-clicks an entity.
#[derive(ServerPacket, Clone, Debug)]
pub struct SInteract {
    pub entity_id: i32,
    pub hand: InteractionHand,
    pub location: DVec3,
    pub using_secondary_action: bool,
}

impl ReadFrom for SInteract {
    fn read(data: &mut std::io::Cursor<&[u8]>) -> std::io::Result<Self> {
        Ok(Self {
            entity_id: VarInt::read(data)?.0,
            hand: InteractionHand::read(data)?,
            location: LpVec3::read(data)?.0,
            using_secondary_action: bool::read(data)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use steel_utils::serial::WriteTo as _;

    use super::*;

    #[test]
    fn interact_packet_reads_vanilla_field_order() {
        let mut bytes = Vec::new();
        bytes.push(42);
        bytes.push(1);
        LpVec3(DVec3::new(0.25, 0.5, 0.75))
            .write(&mut bytes)
            .unwrap();
        bytes.push(1);

        let packet = SInteract::read(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("interact packet should parse: {error}"));

        assert_eq!(packet.entity_id, 42);
        assert_eq!(packet.hand, InteractionHand::OffHand);
        assert!((packet.location.x - 0.25).abs() < 0.0001);
        assert!((packet.location.y - 0.5).abs() < 0.0001);
        assert!((packet.location.z - 0.75).abs() < 0.0001);
        assert!(packet.using_secondary_action);
    }
}
