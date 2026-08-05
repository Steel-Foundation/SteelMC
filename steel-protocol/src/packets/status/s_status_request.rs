use steel_macros::{ReadFrom, ServerPacket};

#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SStatusRequest {}
