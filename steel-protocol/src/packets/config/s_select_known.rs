use steel_macros::{ReadFrom, ServerPacket};

use crate::packets::shared_implementation::KnownPack;

#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SSelectKnownPacks {
    #[read(as = Prefixed(VarInt), bound = 64)]
    pub packs: Vec<KnownPack>,
}
