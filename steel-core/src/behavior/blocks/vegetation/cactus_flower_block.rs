//! Cactus flower block behavior.
//!
//! Cactus flower is a vegetation block that can be placed on cactus, farmland,
//! or any block with a sturdy center face on top.

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::blocks::shapes::SupportType;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess};

/// Behavior for cactus flower blocks.
#[block_behavior]
pub struct CactusFlowerBlock {
    block: BlockRef,
}

impl CactusFlowerBlock {
    /// Creates a new cactus flower block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for CactusFlowerBlock {
    /// Checks if the block below can support a cactus flower.
    ///
    /// Vanilla `CactusFlowerBlock.mayPlaceOn`: accepts the support-override tag
    /// or any block with a sturdy center face on top.
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below = world.get_block_state(below_pos);
        below
            .get_block()
            .has_tag(&BlockTag::SUPPORT_OVERRIDE_CACTUS_FLOWER)
            || below.is_face_sturdy_for_at(below_pos, Direction::Up, SupportType::Center)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.block.default_state();
        self.can_survive(state, context.world, context.place_pos())
            .then_some(state)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if self.can_survive(state, world, pos) {
            state
        } else {
            vanilla_blocks::AIR.default_state()
        }
    }
}
