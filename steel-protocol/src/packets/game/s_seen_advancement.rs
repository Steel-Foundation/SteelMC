use steel_macros::{ReadFrom, ServerPacket};
use steel_utils::Identifier;

#[derive(ReadFrom, ServerPacket, Clone, Debug)]
#[repr(i32)]
pub enum SSeenAdvancement {
    Opened(Identifier) = 0,
    Closed = 1,
}
