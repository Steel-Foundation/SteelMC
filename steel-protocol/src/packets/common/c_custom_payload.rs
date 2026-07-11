use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::config::C_CUSTOM_PAYLOAD;
use steel_registry::packets::play::C_CUSTOM_PAYLOAD as PLAY_C_CUSTOM_PAYLOAD;
use steel_utils::Identifier;

#[derive(WriteTo, ClientPacket, Clone, Debug)]
#[packet_id(Config = C_CUSTOM_PAYLOAD, Play = PLAY_C_CUSTOM_PAYLOAD)]
pub struct CCustomPayload {
    pub identifier: Identifier,
    #[write(as = Prefixed(VarInt))]
    pub payload: Box<[u8]>,
}

impl CCustomPayload {
    #[must_use]
    pub const fn new(identifier: Identifier, payload: Box<[u8]>) -> Self {
        Self {
            identifier,
            payload,
        }
    }
}
