use steel_macros::{ReadFrom, ServerPacket};
use steel_registry::blocks::properties::Direction;
use steel_utils::BlockPos;

/// Action types for the player action packet.
#[derive(ReadFrom, Clone, Copy, Debug, PartialEq, Eq)]
#[read(as = VarInt)]
pub enum PlayerAction {
    StartDestroyBlock = 0,
    AbortDestroyBlock = 1,
    StopDestroyBlock = 2,
    DropAllItems = 3,
    DropItem = 4,
    ReleaseUseItem = 5,
    SwapItemWithOffhand = 6,
    Stab = 7,
}

/// Serverbound packet sent when a player performs an action like mining a block.
#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SPlayerAction {
    pub action: PlayerAction,
    pub pos: BlockPos,
    pub direction: Direction,
    #[read(as = VarInt)]
    pub sequence: i32,
}
