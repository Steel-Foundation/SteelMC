use steel_macros::{ReadFrom, ServerPacket};

#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SKey {
    #[read(as = Prefixed(VarInt))]
    pub key: Vec<u8>,
    #[read(as = Prefixed(VarInt))]
    pub challenge: Vec<u8>,
}

impl SKey {
    #[must_use]
    pub const fn new(key: Vec<u8>, challenge: Vec<u8>) -> Self {
        Self { key, challenge }
    }
}
