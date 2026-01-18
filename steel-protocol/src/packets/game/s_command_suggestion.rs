use steel_macros::{ReadFrom, ServerPacket};
#[allow(unused_imports)]
use steel_registry::packets::play::S_COMMAND_SUGGESTION;

/// Sent by the client when requesting command suggestions (tab completion).
#[derive(ServerPacket, ReadFrom, Clone, Debug)]
#[packet_id(Play = S_COMMAND_SUGGESTION)]
pub struct SCommandSuggestion {
    /// Transaction ID used to match this request with the server's response.
    #[read(as = VarInt)]
    pub id: i32,
    /// The command being typed, including the leading slash.
    #[read(as = Prefixed(VarInt), bound = 32500)]
    pub command: String,
}
