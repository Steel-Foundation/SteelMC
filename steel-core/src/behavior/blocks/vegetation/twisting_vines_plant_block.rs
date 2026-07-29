use rand::Rng;
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::{item_stack::ItemStack, vanilla_blocks, vanilla_items};
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::blocks::vegetation::bonemealable::{BonemealAction, Bonemealable};
use crate::behavior::context::BlockPlaceContext;
use crate::behavior::{
    block::BlockBehavior, blocks::vegetation::growing_plant_body_block::GrowingPlantBodyBlock,
};
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::BlockRef;

/// Vanilla `TwistingVinesPlantBlock` (body) survival.
#[block_behavior]
pub struct TwistingVinesPlantBlock {
    block: BlockRef,
}

impl TwistingVinesPlantBlock {
    /// Creates a new twisting vines plant (body) block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
    const fn growing_plant_body_block(&self) -> GrowingPlantBodyBlock {
        GrowingPlantBodyBlock::new(
            self.block,
            Direction::Up,
            false,
            &vanilla_blocks::TWISTING_VINES,
        )
    }
}
impl BlockBehavior for TwistingVinesPlantBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        self.growing_plant_body_block()
            .can_survive(state, world, pos)
    }
    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.growing_plant_body_block()
            .random_tick(state, world, pos);
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.growing_plant_body_block().update_shape(
            state,
            world,
            pos,
            direction,
            neighbor_pos,
            neighbor_state,
        )
    }
    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.growing_plant_body_block().tick(state, world, pos);
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.growing_plant_body_block()
            .get_state_for_placement(context)
    }
    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        Some(ItemStack::new(&vanilla_items::TWISTING_VINES))
    }
    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}
impl Bonemealable for TwistingVinesPlantBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        self.growing_plant_body_block()
            .is_valid_bonemeal_target(state, world, pos)
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        self.growing_plant_body_block()
            .perform_bonemeal(state, world, rng, pos);
    }

    fn bonemeal_action_type(&self) -> BonemealAction {
        BonemealAction::Grower
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;

    use super::*;
    use crate::test_support::TestLevel;

    #[test]
    fn bonemeal_target_follows_connected_head() {
        init_test_registry();

        let behavior = TwistingVinesPlantBlock::new(&vanilla_blocks::TWISTING_VINES_PLANT);
        let state = vanilla_blocks::TWISTING_VINES_PLANT.default_state();
        let open_level = TestLevel::default().with_block(
            BlockPos::ZERO.above(),
            vanilla_blocks::TWISTING_VINES.default_state(),
        );
        assert!(behavior.is_valid_bonemeal_target(state, &open_level, BlockPos::ZERO));

        let blocked_level = open_level.with_block(
            BlockPos::ZERO.above().above(),
            vanilla_blocks::NETHERRACK.default_state(),
        );
        assert!(!behavior.is_valid_bonemeal_target(state, &blocked_level, BlockPos::ZERO));
    }
}
