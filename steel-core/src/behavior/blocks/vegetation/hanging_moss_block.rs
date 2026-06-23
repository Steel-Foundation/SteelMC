use std::sync::Arc;

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::bonemealable::{BonemealAction, Bonemealable};
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;
use crate::world::{ScheduledTickAccess, World};
use rand::Rng;
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use super::{BlockRef, can_attach_to_multiface, default_surviving_state};

const TIP: BoolProperty = BlockStateProperties::TIP;

/// Vanilla `HangingMossBlock` survival (e.g. `pale_hanging_moss`).
#[block_behavior]
pub struct HangingMossBlock {
    block: BlockRef,
}

impl HangingMossBlock {
    /// Creates a new hanging moss block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
    /// Finds the tip (bottom-most block) of a hanging moss chain.
    /// Returns the position of the lowest hanging moss block in the chain.
    fn get_tip(&self, world: &dyn LevelReader, pos: BlockPos) -> BlockPos {
        let mut forward_pos = pos;
        let mut forward_state;

        loop {
            forward_pos = forward_pos.below();
            forward_state = world.get_block_state(forward_pos);

            if forward_state.get_block() != self.block {
                break;
            }
        }
        forward_pos.above()
    }
    fn can_grow_into(state: BlockStateId) -> bool {
        state.is_air()
    }
}

impl BlockBehavior for HangingMossBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        // Vanilla `canStayAtPosition`: the block above either attaches via the
        // multiface rule (support OR collision face full) or is more hanging
        // moss of the same kind.
        let above_pos = pos.above();
        if can_attach_to_multiface(world, above_pos, Direction::Up) {
            return true;
        }
        world.get_block_state(above_pos).get_block() == self.block
    }
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: steel_utils::Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if !self.can_survive(state, world, pos) {
            world.schedule_block_tick_default(pos, self.block, 1);
        }
        let is_tip = world.get_block_state(pos.below()).get_block() != self.block;
        state.set_value(&TIP, is_tip)
    }
    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !self.can_survive(state, world, pos) {
            world.destroy_block(pos, true);
        }
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}
impl Bonemealable for HangingMossBlock {
    fn is_valid_bonemeal_target(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        let grow_pos = self.get_tip(world, pos).below();
        HangingMossBlock::can_grow_into(world.get_block_state(grow_pos))
            && !world.is_outside_build_height(grow_pos.y())
    }

    fn is_bonemeal_success(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _rng: &mut dyn Rng,
        _pos: BlockPos,
    ) -> bool {
        true
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        let tip_pos = self.get_tip(world, pos).below();
        if HangingMossBlock::can_grow_into(world.get_block_state(tip_pos)) {
            world.set_block(
                tip_pos,
                state.set_value(&TIP, true),
                UpdateFlags::UPDATE_ALL,
            );
        }
    }

    fn bonemeal_action_type(&self) -> BonemealAction {
        BonemealAction::Grower
    }
}
