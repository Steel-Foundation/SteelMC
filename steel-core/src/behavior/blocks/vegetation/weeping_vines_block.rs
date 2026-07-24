use rand::Rng;
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::blocks::vegetation::bonemealable::{BonemealAction, Bonemealable};
use crate::behavior::blocks::vegetation::growing_plant_head_block::GrowingPlantHeadBlock;
use crate::behavior::blocks::vegetation::nether_vines_get_blocks_to_grow_when_bonemealed;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, World};
use crate::{behavior::block::BlockBehavior, world::ScheduledTickAccess};

use super::BlockRef;

/// Vanilla `WeepingVinesBlock` (head) survival.
#[block_behavior]
pub struct WeepingVinesBlock {
    block: BlockRef,
}

impl WeepingVinesBlock {
    /// Creates a new weeping vines (head) block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
    const fn growing_plant_head_block(&self) -> GrowingPlantHeadBlock {
        GrowingPlantHeadBlock::new(
            self.block,
            Direction::Down,
            false,
            0.1,
            &vanilla_blocks::WEEPING_VINES_PLANT,
            Some(nether_vines_get_blocks_to_grow_when_bonemealed),
        )
    }
}

impl BlockBehavior for WeepingVinesBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        self.growing_plant_head_block()
            .can_survive(state, world, pos)
    }
    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.growing_plant_head_block()
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
        self.growing_plant_head_block().update_shape(
            state,
            world,
            pos,
            direction,
            neighbor_pos,
            neighbor_state,
        )
    }
    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.growing_plant_head_block().tick(state, world, pos);
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.growing_plant_head_block()
            .get_state_for_placement(context)
    }
    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}
impl Bonemealable for WeepingVinesBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        self.growing_plant_head_block()
            .is_valid_bonemeal_target(state, world, pos)
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        self.growing_plant_head_block()
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
    fn bonemeal_target_requires_open_growth_position() {
        init_test_registry();

        let behavior = WeepingVinesBlock::new(&vanilla_blocks::WEEPING_VINES);
        let state = vanilla_blocks::WEEPING_VINES.default_state();
        let open_level = TestLevel::default();
        assert!(behavior.is_valid_bonemeal_target(state, &open_level, BlockPos::ZERO));

        let blocked_level = TestLevel::default().with_block(
            BlockPos::ZERO.below(),
            vanilla_blocks::NETHERRACK.default_state(),
        );
        assert!(!behavior.is_valid_bonemeal_target(state, &blocked_level, BlockPos::ZERO));
    }

    #[test]
    fn connected_head_converts_to_body() {
        init_test_registry();

        let behavior = WeepingVinesBlock::new(&vanilla_blocks::WEEPING_VINES);
        let state = vanilla_blocks::WEEPING_VINES.default_state();
        let level = TestLevel::default();
        let converted = behavior.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::Down,
            BlockPos::ZERO.below(),
            vanilla_blocks::WEEPING_VINES_PLANT.default_state(),
        );

        assert_eq!(
            converted,
            vanilla_blocks::WEEPING_VINES_PLANT.default_state()
        );
    }
}
