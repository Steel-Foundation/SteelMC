//! Cactus flower block behavior.
//!
//! Cactus flower is a vegetation block that can be placed on cactus, farmland,
//! or any block with a sturdy center face on top.
//!
//! Vanilla equivalent: `CactusFlowerBlock` extends `VegetationBlock`.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::blocks::shapes::SupportType;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::World;

/// Behavior for cactus flower blocks.
///
/// Cactus flower can be placed on cactus, farmland, or any block with
/// a sturdy center face on top. Breaks instantly if the supporting block
/// is removed (returns AIR from `update_shape`).
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
    /// Vanilla `CactusFlowerBlock.mayPlaceOn`: accepts CACTUS, FARMLAND,
    /// or any block with a sturdy center face on top.
    fn can_survive(&self, _state: BlockStateId, world: &Arc<World>, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below = world.get_block_state(below_pos);
        let below_block = below.get_block();

        below_block == vanilla_blocks::CACTUS
            || below_block == vanilla_blocks::FARMLAND
            || below.is_face_sturdy_for(Direction::Up, SupportType::Center)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let default_state = self.block.default_state();
        if self.can_survive(default_state, context.world, context.relative_pos) {
            Some(default_state)
        } else {
            None
        }
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if !self.can_survive(state, world, pos) {
            return vanilla_blocks::AIR.default_state();
        }
        state
    }
}
