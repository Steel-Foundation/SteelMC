use steel_macros::{ReadFrom, ServerPacket};

#[derive(Debug, Clone, ReadFrom, ServerPacket)]
pub struct SLoginAcknowledged {}
