use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::fluid::{FluidRef, FluidState};
use steel_registry::{vanilla_blocks, vanilla_fluids};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::{BlockRef, kelp_can_survive, water_source_fluid_state};

/// Vanilla `KelpBlock` survival and fluid state.
// TODO: Implement random growth, bonemeal growth, and clone stack behavior.
#[block_behavior]
pub struct KelpBlock {
    block: BlockRef,
}

impl KelpBlock {
    /// Creates a new kelp block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn body_state() -> BlockStateId {
        vanilla_blocks::KELP_PLANT.default_state()
    }
}

impl BlockBehavior for KelpBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        kelp_can_survive(world, pos)
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
        if direction == Direction::Down {
            if self.can_survive(state, world, pos) {
                let above = world.get_block_state(pos.above());
                let above_block = above.get_block();
                if above_block == self.block || above_block == &vanilla_blocks::KELP_PLANT {
                    return Self::body_state();
                }
            } else {
                let _ = world.schedule_block_tick_default(pos, self.block, 1);
            }
        }

        let neighbor_block = neighbor_state.get_block();
        if direction == Direction::Up
            && (neighbor_block == self.block || neighbor_block == &vanilla_blocks::KELP_PLANT)
        {
            Self::body_state()
        } else {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
            state
        }
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if !context.is_full_water() {
            return None;
        }

        let above = context.world.get_block_state(context.relative_pos.above());
        let above_block = above.get_block();
        if above_block == self.block || above_block == &vanilla_blocks::KELP_PLANT {
            let state = Self::body_state();
            return self
                .can_survive(state, context.world, context.relative_pos)
                .then_some(state);
        }

        // Intentional Steel divergence: incidental runtime age does not use world RNG.
        let age = rand::random_range(0..25) as u8;
        let state = self
            .block
            .default_state()
            .set_value(&BlockStateProperties::AGE_25, age);
        self.can_survive(state, context.world, context.relative_pos)
            .then_some(state)
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !self.can_survive(state, world, pos) {
            world.destroy_block(pos, true);
        }
    }

    fn get_fluid_state(&self, _state: BlockStateId) -> FluidState {
        water_source_fluid_state()
    }

    fn is_liquid_container(&self, _state: BlockStateId) -> bool {
        true
    }

    fn can_place_liquid(&self, _state: BlockStateId, _fluid: FluidRef) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestLevel;
    use steel_registry::test_support::init_test_registry;

    #[test]
    fn kelp_update_shape_schedules_water_tick() {
        init_test_registry();

        let kelp = KelpBlock::new(&vanilla_blocks::KELP);
        let level =
            TestLevel::default().with_default_block_state(vanilla_blocks::WATER.default_state());
        let state = vanilla_blocks::KELP.default_state();

        assert_eq!(
            kelp.update_shape(
                state,
                &level,
                BlockPos::ZERO,
                Direction::North,
                Direction::North.relative(BlockPos::ZERO),
                vanilla_blocks::WATER.default_state(),
            ),
            state
        );
        assert!(level.scheduled_water_tick());
    }

    #[test]
    fn kelp_head_update_shape_schedules_break_tick_when_unsupported() {
        init_test_registry();

        let kelp = KelpBlock::new(&vanilla_blocks::KELP);
        let level =
            TestLevel::default().with_default_block_state(vanilla_blocks::WATER.default_state());
        let state = vanilla_blocks::KELP.default_state();

        let updated = kelp.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::Down,
            BlockPos::ZERO.below(),
            vanilla_blocks::WATER.default_state(),
        );

        assert_eq!(updated, state);
        assert!(
            level
                .scheduled_block_ticks
                .borrow()
                .iter()
                .any(|tick| tick.block == &vanilla_blocks::KELP && tick.delay == 1)
        );
    }

    #[test]
    fn kelp_head_converts_to_body_when_connected_above() {
        init_test_registry();

        let kelp = KelpBlock::new(&vanilla_blocks::KELP);
        let level =
            TestLevel::default().with_default_block_state(vanilla_blocks::WATER.default_state());
        let state = vanilla_blocks::KELP.default_state();

        let updated = kelp.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::Up,
            BlockPos::ZERO.above(),
            vanilla_blocks::KELP_PLANT.default_state(),
        );

        assert_eq!(updated.get_block(), &vanilla_blocks::KELP_PLANT);
        assert!(level.scheduled_fluid_ticks.borrow().is_empty());
    }
}
