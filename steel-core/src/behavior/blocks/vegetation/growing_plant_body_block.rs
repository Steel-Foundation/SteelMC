use rand::{RngExt, rng};
use std::sync::Arc;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, IntProperty},
    },
    vanilla_fluids,
};
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext,
        blocks::vegetation::{
            growing_plant_can_survive, growing_plant_head_block::GrowingPlantHeadBlock,
        },
    },
    world::{LevelReader, ScheduledTickAccess, World},
};

/// Shared behavior for growing plant blocks.
pub struct GrowingPlantBodyBlock {
    block: BlockRef,
    growth_direction: Direction,
    schedule_fluid_ticks: bool,
    head_block: BlockRef,
}
const AGE: IntProperty = BlockStateProperties::AGE_25;

impl GrowingPlantBodyBlock {
    /// Creates a new leaves block behavior.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        growth_direction: Direction,
        schedule_fluid_ticks: bool,
        head_block: BlockRef,
    ) -> Self {
        Self {
            block,
            growth_direction,
            schedule_fluid_ticks,
            head_block,
        }
    }
    const fn update_head_after_converted_from_body(
        _body_state: BlockStateId,
        head_state: BlockStateId,
    ) -> BlockStateId {
        head_state
    }
}

impl BlockBehavior for GrowingPlantBodyBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        growing_plant_can_survive(
            world,
            pos,
            self.growth_direction,
            self.head_block,
            state.get_block(),
        )
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if direction == self.growth_direction.opposite() && !self.can_survive(state, world, pos) {
            world.schedule_block_tick_default(pos, self.block, 1);
        }
        let head_block = self.head_block;
        if direction == self.growth_direction
            && neighbor_state.get_block() != self.block
            && neighbor_state.get_block() != head_block
        {
            return Self::update_head_after_converted_from_body(
                state,
                GrowingPlantHeadBlock::get_head_state(self.head_block),
            );
        }
        if self.schedule_fluid_ticks {
            world.schedule_fluid_tick_default(
                pos,
                &vanilla_fluids::WATER,
                vanilla_fluids::WATER.tick_delay as i32,
            );
        }
        state
    }
    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !self.can_survive(state, world, pos) {
            world.destroy_block(pos, true);
        }
    }
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(
            self.block
                .default_state()
                .set_value(&AGE, rng().random_range(0..25)),
        )
    }
}
