use rand::{RngExt, rng};
use std::sync::Arc;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, IntProperty, Property},
    },
    vanilla_fluids,
};
use steel_utils::{BlockPos, BlockStateId, Direction, types::UpdateFlags};

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext, blocks::vegetation::growing_plant_can_survive},
    world::{LevelAccessor, LevelReader, ScheduledTickAccess, World},
};

/// Shared behavior for growing plant head blocks.
pub struct GrowingPlantHeadBlock {
    block: BlockRef,
    growth_direction: Direction,
    schedule_fluid_ticks: bool,
    grow_per_tick_probability: f64,
    body_block: BlockRef,
}
const AGE: IntProperty = BlockStateProperties::AGE_25;

impl GrowingPlantHeadBlock {
    /// Creates a new leaves block behavior.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        growth_direction: Direction,
        schedule_fluid_ticks: bool,
        grow_per_tick_probability: f64,
        body_block: BlockRef,
    ) -> Self {
        Self {
            block,
            growth_direction,
            schedule_fluid_ticks,
            grow_per_tick_probability,
            body_block,
        }
    }
    fn get_grow_into_state(grow_from_state: BlockStateId) -> BlockStateId {
        //basically find next age value. vanilla equivalent to growFromState.cycle(AGE)
        let values = AGE.get_possible_values();
        let current = grow_from_state.get_value(&AGE);

        let Some(next_age) = values
            .iter()
            .position(|v| *v == current)
            .map(|i| values[(i + 1) % values.len()])
        else {
            return grow_from_state;
        };
        grow_from_state.set_value(&AGE, next_age)
    }
    fn can_grow_into(state: BlockStateId) -> bool {
        state.is_air()
    }
    const fn update_body_after_converted_from_head(
        _head_state: BlockStateId,
        body_state: BlockStateId,
    ) -> BlockStateId {
        body_state
    }
    pub fn get_head_state(block: BlockRef) -> BlockStateId {
        block
            .default_state()
            .set_value(&AGE, rng().random_range(0..25))
    }
}

impl BlockBehavior for GrowingPlantHeadBlock {
    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        state.get_value(&AGE) < 25
    }
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        growing_plant_can_survive(
            world,
            pos,
            self.growth_direction,
            state.get_block(),
            self.body_block,
        )
    }
    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if state.get_value(&AGE) < 25 && rng().random::<f64>() < self.grow_per_tick_probability {
            let growth_pos = pos.relative(self.growth_direction);
            if Self::can_grow_into(world.get_block_state(growth_pos)) {
                world.set_block_state(
                    growth_pos,
                    Self::get_grow_into_state(state),
                    UpdateFlags::UPDATE_ALL,
                );
            }
        }
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
        if direction == self.growth_direction.opposite() {
            if self.can_survive(state, world, pos) {
                let neighbor_in_growth_direction =
                    world.get_block_state(pos.relative(self.growth_direction));
                if neighbor_in_growth_direction.get_block() == self.block
                    || neighbor_in_growth_direction.get_block() == self.body_block
                {
                    return Self::update_body_after_converted_from_head(
                        state,
                        self.body_block.default_state(),
                    );
                }
            } else {
                world.schedule_block_tick_default(pos, self.block, 1);
            }
        }
        if direction != self.growth_direction
            || neighbor_state.get_block() != self.block
                && neighbor_state.get_block() != self.body_block
        {
            if self.schedule_fluid_ticks {
                world.schedule_fluid_tick_default(
                    pos,
                    &vanilla_fluids::WATER,
                    vanilla_fluids::WATER.tick_delay as i32,
                );
            }
            return state;
        }
        Self::update_body_after_converted_from_head(state, self.body_block.default_state())
    }
    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !self.can_survive(state, world, pos) {
            world.destroy_block(pos, true);
        }
    }
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(Self::get_head_state(self.block))
    }
}
