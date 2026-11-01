//! Player block and interaction events.

use std::sync::Arc;

use steel_utils::{BlockPos, BlockStateId, Direction, Identifier, types::InteractionHand};

use super::Event;
use crate::{player::Player, world::World};

/// A player is about to break a block. Cancelling keeps the block and resyncs the client.
pub struct BlockBreakEvent {
    /// Breaking player.
    pub player: Arc<Player>,
    /// Player's world.
    pub world: Arc<World>,
    /// Broken block.
    pub pos: BlockPos,
    /// State before breaking.
    pub state: BlockStateId,
    /// Whether loot and experience drop (Bukkit `setDropItems`).
    pub drop_items: bool,
    /// Whether the block stays.
    pub cancelled: bool,
}

impl Event for BlockBreakEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A player is about to place a block after every vanilla placement check passed.
pub struct BlockPlaceEvent {
    /// Placing player.
    pub player: Arc<Player>,
    /// Player's world.
    pub world: Arc<World>,
    /// Placement position.
    pub pos: BlockPos,
    /// State to place.
    pub state: BlockStateId,
    /// Clicked block the placement is against.
    pub against: BlockPos,
    /// Whether the placement fails.
    pub cancelled: bool,
}

impl Event for BlockPlaceEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// Which click caused an interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InteractAction {
    /// Started breaking (left click) a block.
    LeftClickBlock,
    /// Used an item on (right click) a block.
    RightClickBlock,
    /// Used an item without targeting a block.
    RightClickAir,
}

/// A player click. Cancelling skips all vanilla handling of the click.
pub struct PlayerInteractEvent {
    /// Clicking player.
    pub player: Arc<Player>,
    /// Player's world.
    pub world: Arc<World>,
    /// Click kind.
    pub action: InteractAction,
    /// Hand used.
    pub hand: InteractionHand,
    /// Clicked block, if any.
    pub pos: Option<BlockPos>,
    /// Clicked face, if any.
    pub face: Option<Direction>,
    /// Held item key (`minecraft:air` when empty).
    pub item: Identifier,
    /// Whether the click is ignored.
    pub cancelled: bool,
}

impl Event for PlayerInteractEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}
