use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::{vanilla_blocks, vanilla_fluids};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::{BlockRef, coral_plant_can_survive, coral_scan_for_water, schedule_coral_die_tick};

/// Vanilla `CoralPlantBlock` survival (live coral plants such as `tube_coral`).
///
/// Inherits `canSurvive` from `BaseCoralPlantTypeBlock`.
#[block_behavior]
pub struct CoralPlantBlock {
    block: BlockRef,
    #[json_arg(vanilla_blocks, json = "dead_block")]
    dead_block: BlockRef,
}

impl CoralPlantBlock {
    /// Creates a new live coral plant block behavior.
    #[must_use]
    pub const fn new(block: BlockRef, dead_block: BlockRef) -> Self {
        Self { block, dead_block }
    }

    fn dead_state(&self) -> BlockStateId {
        self.dead_block
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, false)
    }
}

impl BlockBehavior for CoralPlantBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        coral_plant_can_survive(world, pos)
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        schedule_coral_die_tick(state, world, pos, self.block);
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
        if direction == Direction::Down && !self.can_survive(state, world, pos) {
            return vanilla_blocks::AIR.default_state();
        }

        schedule_coral_die_tick(state, world, pos, self.block);

        if state.get_value(&BlockStateProperties::WATERLOGGED) {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }

        state
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.block.default_state();
        if !self.can_survive(state, context.world, context.relative_pos) {
            return None;
        }
        // Vanilla: WATERLOGGED reflects whether the click position has full water.
        Some(state.set_value(&BlockStateProperties::WATERLOGGED, context.is_full_water()))
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !coral_scan_for_water(state, world, pos) {
            world.set_block(pos, self.dead_state(), UpdateFlags::UPDATE_CLIENTS);
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;

    use crate::behavior::init_behaviors;
    use crate::test_support::TestLevel;

    use super::*;

    fn supported_level() -> TestLevel {
        TestLevel::default().with_block(
            BlockPos::ZERO.below(),
            vanilla_blocks::STONE.default_state(),
        )
    }

    #[test]
    fn dry_coral_plant_update_shape_schedules_die_tick() {
        init_test_registry();
        init_behaviors();
        let behavior = CoralPlantBlock::new(
            &vanilla_blocks::TUBE_CORAL,
            &vanilla_blocks::DEAD_TUBE_CORAL,
        );
        let level = supported_level();
        let state = vanilla_blocks::TUBE_CORAL
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, false);

        assert_eq!(
            behavior.update_shape(
                state,
                &level,
                BlockPos::ZERO,
                Direction::North,
                BlockPos::ZERO.north(),
                vanilla_blocks::AIR.default_state(),
            ),
            state
        );

        let scheduled = level.scheduled_block_ticks.borrow();
        assert_eq!(scheduled.len(), 1);
        assert_eq!(scheduled[0].block, &vanilla_blocks::TUBE_CORAL);
        assert!((60..100).contains(&scheduled[0].delay));
    }

    #[test]
    fn waterlogged_coral_plant_update_shape_schedules_water_not_die_tick() {
        init_test_registry();
        init_behaviors();
        let behavior = CoralPlantBlock::new(
            &vanilla_blocks::TUBE_CORAL,
            &vanilla_blocks::DEAD_TUBE_CORAL,
        );
        let level = supported_level();
        let state = vanilla_blocks::TUBE_CORAL
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, true);

        assert_eq!(
            behavior.update_shape(
                state,
                &level,
                BlockPos::ZERO,
                Direction::North,
                BlockPos::ZERO.north(),
                vanilla_blocks::AIR.default_state(),
            ),
            state
        );

        assert!(level.scheduled_block_ticks.borrow().is_empty());
        assert!(level.scheduled_water_tick());
    }
}
