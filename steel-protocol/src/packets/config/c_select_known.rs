use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::config::C_SELECT_KNOWN_PACKS;

use crate::packets::shared_implementation::KnownPack;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Config = C_SELECT_KNOWN_PACKS)]
pub struct CSelectKnownPacks {
    #[write(as = Prefixed(VarInt))]
    pub packs: Vec<KnownPack>,
}

impl CSelectKnownPacks {
    #[must_use]
    pub const fn new(packs: Vec<KnownPack>) -> Self {
        Self { packs }
    }
}
