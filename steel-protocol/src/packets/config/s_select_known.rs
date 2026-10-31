use steel_macros::{ReadFrom, ServerPacket};

use crate::packets::shared_implementation::KnownPack;

#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SSelectKnownPacks {
    #[read(as = Prefixed(VarInt), bound = 64)]
    pub packs: Vec<KnownPack>,
}

#[cfg(test)]
mod tests {
    use super::SSelectKnownPacks;
    use crate::packets::shared_implementation::KnownPack;
    use std::io::Cursor;
    use steel_utils::codec::VarInt;
    use steel_utils::serial::{ReadFrom, WriteTo};

    fn encode_packs(count: i32) -> Vec<u8> {
        let mut data = Vec::new();
        VarInt(count).write(&mut data).unwrap();

        for _ in 0..count {
            let pack = KnownPack::new(String::new(), String::new(), String::new());
            pack.write(&mut data).unwrap();
        }
        data
    }

    #[test]
    fn accepts_64_packs() {
        let data = encode_packs(64);
        let mut cursor = Cursor::new(data.as_slice());

        let packet = SSelectKnownPacks::read(&mut cursor)
            .expect("64 known packs should decode successfully");

        assert_eq!(packet.packs.len(), 64);
    }

    #[test]
    fn rejects_65_packs() {
        let data = encode_packs(65);
        let mut cursor = Cursor::new(data.as_slice());
        assert!(SSelectKnownPacks::read(&mut cursor).is_err());
    }
}
