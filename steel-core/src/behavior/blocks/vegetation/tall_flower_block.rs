use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::properties::Direction;
use steel_registry::item_stack::ItemStack;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::player::Player;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::{BlockRef, DoublePlantBlock};

/// Vanilla `TallFlowerBlock` survival.
// TODO: Implement bonemeal and the rest of vanilla behavior.
#[block_behavior]
pub struct TallFlowerBlock {
    base: DoublePlantBlock,
}

impl TallFlowerBlock {
    /// Creates a new tall flower block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            base: DoublePlantBlock::new(block),
        }
    }
}

impl BlockBehavior for TallFlowerBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.base
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        self.base.can_survive(state, world, pos)
    }

    fn set_placed_by(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: Option<&Player>,
        item_stack: &ItemStack,
    ) {
        self.base
            .set_placed_by(state, world, pos, player, item_stack);
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.base.get_state_for_placement(context)
    }
}
