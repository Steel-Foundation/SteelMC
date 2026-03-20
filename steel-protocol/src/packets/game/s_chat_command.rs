use steel_macros::{ReadFrom, ServerPacket};
#[expect(unused_imports)]
use steel_registry::packets::play::S_CHAT_COMMAND;

#[derive(ServerPacket, ReadFrom)]
#[packet_id(Play = S_CHAT_COMMAND)]
pub struct SChatCommand {
    #[read(as = Prefixed(VarInt))]
    pub command: String,
}
