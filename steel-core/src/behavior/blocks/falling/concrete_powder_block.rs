use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::properties::Direction;
use steel_registry::blocks::{BlockRef, block_state_ext::BlockStateExt as _};
use steel_registry::fluid::FluidStateExt as _;
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::{BlockBehavior, BlockPlaceContext, Fallable};
use crate::entity::entities::FallingBlockEntity;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::FallingBlock;

/// Vanilla `ConcretePowderBlock` behavior.
#[block_behavior]
pub struct ConcretePowderBlock {
    falling: FallingBlock,
    #[json_arg(vanilla_blocks, json = "concrete")]
    concrete: BlockRef,
}

impl ConcretePowderBlock {
    /// Creates concrete powder with its extractor-owned solid block mapping.
    #[must_use]
    pub const fn new(block: BlockRef, concrete: BlockRef) -> Self {
        Self {
            falling: FallingBlock::new(block),
            concrete,
        }
    }

    fn can_solidify(state: BlockStateId) -> bool {
        state.get_fluid_state().is_water()
    }

    fn touches_liquid(world: &dyn LevelReader, pos: BlockPos) -> bool {
        let mut test_pos = pos;
        for direction in Direction::ALL {
            let state_at_test_pos = world.get_block_state(test_pos);
            if direction == Direction::Down && !Self::can_solidify(state_at_test_pos) {
                continue;
            }

            test_pos = pos.relative(direction);
            let neighbor_state = world.get_block_state(test_pos);
            if Self::can_solidify(neighbor_state)
                && !world.is_face_sturdy(neighbor_state, pos, direction.opposite())
            {
                return true;
            }
        }
        false
    }

    fn should_solidify(
        world: &dyn LevelReader,
        pos: BlockPos,
        replaced_state: BlockStateId,
    ) -> bool {
        Self::can_solidify(replaced_state) || Self::touches_liquid(world, pos)
    }
}

impl Fallable for ConcretePowderBlock {
    fn on_land(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        _state: BlockStateId,
        replaced_state: BlockStateId,
        _entity: &FallingBlockEntity,
    ) {
        if Self::should_solidify(world.as_ref(), pos, replaced_state) {
            world.set_block(pos, self.concrete.default_state(), UpdateFlags::UPDATE_ALL);
        }
    }

    fn is_concrete_powder(&self) -> bool {
        true
    }
}

impl BlockBehavior for ConcretePowderBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let pos = context.place_pos();
        let replaced_state = context.world.get_block_state(pos);
        if Self::should_solidify(context.world.as_ref(), pos, replaced_state) {
            Some(self.concrete.default_state())
        } else {
            Some(self.falling.block().default_state())
        }
    }

    fn on_place(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        self.falling.on_place(world, pos);
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        ticks: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if Self::touches_liquid(ticks, pos) {
            self.concrete.default_state()
        } else {
            self.falling.update_shape(state, ticks, pos)
        }
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let _ = FallingBlock::tick(state, world, pos);
    }

    fn as_fallable(&self) -> Option<&dyn Fallable> {
        Some(self)
    }
}
