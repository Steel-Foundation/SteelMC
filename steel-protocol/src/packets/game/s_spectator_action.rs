use std::io::{Cursor, Result};

use steel_macros::ServerPacket;
use steel_utils::codec::VarInt;
use steel_utils::serial::ReadFrom;

#[derive(ServerPacket, Clone, Debug)]
pub struct SSpectatorAction {
    pub spectate_entity_id: Option<i32>,
}

impl ReadFrom for SSpectatorAction {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let spectate_entity_id = Option::<VarInt>::read(data)?.map(|id| id.0);
        Ok(Self { spectate_entity_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_absent_entity_id() {
        let mut data = Cursor::new([0].as_slice());
        let packet = SSpectatorAction::read(&mut data).expect("packet should parse");

        assert_eq!(packet.spectate_entity_id, None);
    }

    #[test]
    fn reads_present_entity_id_as_varint() {
        let mut data = Cursor::new([1, 62].as_slice());
        let packet = SSpectatorAction::read(&mut data).expect("packet should parse");

        assert_eq!(packet.spectate_entity_id, Some(62));
    }
}
