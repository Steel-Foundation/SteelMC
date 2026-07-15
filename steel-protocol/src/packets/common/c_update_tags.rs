use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::config::C_UPDATE_TAGS;
use steel_registry::packets::play::C_UPDATE_TAGS as PLAY_C_UPDATE_TAGS;
use steel_utils::Identifier;
use steel_utils::codec::VarInt;

pub type TagCollection = Vec<(Identifier, Vec<(Identifier, Vec<VarInt>)>)>;

#[derive(ClientPacket, WriteTo)]
#[packet_id(Config = C_UPDATE_TAGS, Play = PLAY_C_UPDATE_TAGS)]
pub struct CUpdateTags {
    pub tags: TagCollection,
}

impl CUpdateTags {
    #[must_use]
    pub const fn new(tags: TagCollection) -> Self {
        Self { tags }
    }
}
