use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_LIGHT_UPDATE;

use super::LightUpdatePacketData;

#[derive(ClientPacket, Debug, Clone, WriteTo)]
#[packet_id(Play = C_LIGHT_UPDATE)]
pub struct CLightUpdate {
    #[write(as = VarInt)]
    pub x: i32,
    #[write(as = VarInt)]
    pub z: i32,
    pub light_data: LightUpdatePacketData,
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use steel_utils::{
        codec::{BitSet, VarInt},
        serial::{ReadFrom, WriteTo},
    };

    use super::CLightUpdate;
    use crate::packets::game::LightUpdatePacketData;

    #[test]
    fn writes_chunk_coordinates_as_varints_and_trims_empty_masks() {
        fn empty_mask() -> BitSet {
            BitSet(vec![0].into_boxed_slice())
        }

        let packet = CLightUpdate {
            x: 2,
            z: -3,
            light_data: LightUpdatePacketData {
                sky_y_mask: empty_mask(),
                block_y_mask: empty_mask(),
                empty_sky_y_mask: empty_mask(),
                empty_block_y_mask: empty_mask(),
                sky_updates: Vec::new(),
                block_updates: Vec::new(),
            },
        };

        let mut data = Vec::new();
        let Ok(()) = packet.write(&mut data) else {
            panic!("light update packet should encode");
        };

        let mut cursor = Cursor::new(data.as_slice());
        let Ok(x) = VarInt::read(&mut cursor) else {
            panic!("x varint missing");
        };
        let Ok(z) = VarInt::read(&mut cursor) else {
            panic!("z varint missing");
        };
        assert_eq!(x.0, 2);
        assert_eq!(z.0, -3);
        for _ in 0..4 {
            let Ok(mask_len) = VarInt::read(&mut cursor) else {
                panic!("light mask length missing");
            };
            assert_eq!(mask_len.0, 0);
        }
    }
}
