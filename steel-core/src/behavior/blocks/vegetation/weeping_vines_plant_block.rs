use rand::Rng;
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::blocks::vegetation::bonemealable::BonemealAction;
use crate::behavior::blocks::vegetation::growing_plant_body_block::GrowingPlantBodyBlock;
use crate::behavior::context::BlockPlaceContext;
use crate::behavior::{block::BlockBehavior, blocks::vegetation::bonemealable::Bonemealable};
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::BlockRef;

/// Vanilla `WeepingVinesPlantBlock` (body) survival.
#[block_behavior]
pub struct WeepingVinesPlantBlock {
    block: BlockRef,
}

impl WeepingVinesPlantBlock {
    /// Creates a new weeping vines plant (body) block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
    const fn growing_plant_body_block(&self) -> GrowingPlantBodyBlock {
        GrowingPlantBodyBlock::new(
            self.block,
            Direction::Down,
            false,
            &vanilla_blocks::WEEPING_VINES,
        )
    }
}

impl BlockBehavior for WeepingVinesPlantBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        self.growing_plant_body_block()
            .can_survive(state, world, pos)
    }
    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        self.growing_plant_body_block().is_randomly_ticking(state)
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
    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}
impl Bonemealable for WeepingVinesPlantBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        self.growing_plant_body_block()
            .as_bonemealable()
            .expect("failed to get weeping_vines_plant_block as bonemealable")
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
            .as_bonemealable()
            .expect("failed to get weeping_vines_plant_block as bonemealable")
            .perform_bonemeal(state, world, rng, pos);
    }

    fn bonemeal_action_type(&self) -> BonemealAction {
        BonemealAction::Grower
    }
}
