//! Vanilla piston-head behavior.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, PistonType};
use steel_registry::item_stack::ItemStack;
use steel_registry::{vanilla_blocks, vanilla_items};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::entity::ai::path::PathComputationType;
use crate::player::Player;
use crate::world::{LevelReader, ScheduledTickAccess, World};

#[block_behavior]
/// Vanilla piston-head block.
pub struct PistonHeadBlock;

impl PistonHeadBlock {
    /// Creates piston-head behavior.
    #[must_use]
    pub const fn new(_block: BlockRef) -> Self {
        Self
    }

    fn is_fitting_base(arm_state: BlockStateId, potential_base: BlockStateId) -> bool {
        let base_block = match arm_state.get_value(&BlockStateProperties::PISTON_TYPE) {
            PistonType::Normal => &vanilla_blocks::PISTON,
            PistonType::Sticky => &vanilla_blocks::STICKY_PISTON,
        };
        potential_base.get_block() == base_block
            && potential_base.get_value(&BlockStateProperties::EXTENDED)
            && potential_base.get_value(&BlockStateProperties::FACING)
                == arm_state.get_value(&BlockStateProperties::FACING)
    }
}

impl BlockBehavior for PistonHeadBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(vanilla_blocks::PISTON_HEAD.default_state())
    }

    fn player_will_destroy(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
    ) -> BlockStateId {
        if player.has_infinite_materials() {
            let base_pos = pos.relative(state.get_value(&BlockStateProperties::FACING).opposite());
            if Self::is_fitting_base(state, world.get_block_state(base_pos)) {
                world.destroy_block(base_pos, false);
            }
        }
        state
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _moved_by_piston: bool,
    ) {
        let base_pos = pos.relative(state.get_value(&BlockStateProperties::FACING).opposite());
        if Self::is_fitting_base(state, world.get_block_state(base_pos)) {
            world.destroy_block(base_pos, true);
        }
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if direction.opposite() == state.get_value(&BlockStateProperties::FACING)
            && !self.can_survive(state, world, pos)
        {
            vanilla_blocks::AIR.default_state()
        } else {
            state
        }
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let facing = state.get_value(&BlockStateProperties::FACING);
        let base = world.get_block_state(pos.relative(facing.opposite()));
        Self::is_fitting_base(state, base)
            || (base.get_block() == &vanilla_blocks::MOVING_PISTON
                && base.get_value(&BlockStateProperties::FACING) == facing)
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        if self.can_survive(state, world.as_ref(), pos) {
            let base_pos = pos.relative(state.get_value(&BlockStateProperties::FACING).opposite());
            world.neighbor_changed(base_pos, source_block);
        }
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        Some(ItemStack::new(
            match state.get_value(&BlockStateProperties::PISTON_TYPE) {
                PistonType::Normal => &vanilla_items::PISTON,
                PistonType::Sticky => &vanilla_items::STICKY_PISTON,
            },
        ))
    }

    fn is_pathfindable(&self, _state: BlockStateId, _type: PathComputationType) -> bool {
        false
    }
}
