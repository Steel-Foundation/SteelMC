use steel_macros::{ReadFrom, ServerPacket};
use steel_utils::types::GameType;

#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SChangeGameMode {
    pub gamemode: GameType,
}
