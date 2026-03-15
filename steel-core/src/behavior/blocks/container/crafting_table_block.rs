//! Crafting table block behavior implementation.
//!
//! Opens the 3x3 crafting grid when right-clicked.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::inventory::CraftingMenuProvider;
use crate::player::Player;
use crate::world::World;

/// Behavior for the crafting table block.
///
/// When a player interacts with the crafting table without an item (or with
/// an item that doesn't consume the action), it opens the 3x3 crafting menu.
#[block_behavior]
pub struct CraftingTableBlock {
    block: BlockRef,
}

impl CraftingTableBlock {
    /// Creates a new crafting table block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for CraftingTableBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn use_without_item(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
    ) -> InteractionResult {
        player.open_menu(&CraftingMenuProvider::new(player.inventory.clone(), pos));
        // TODO: Award stat INTERACT_WITH_CRAFTING_TABLE
        InteractionResult::Success
    }
}
