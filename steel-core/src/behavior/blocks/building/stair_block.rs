//! Stair block behavior implementation.
//!
//! Stairs recompute their `shape` property from adjacent stairs during placement
//! and horizontal neighbor updates.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{
    BlockStateProperties, Direction, EnumProperty, Half, StairsShape,
};
use steel_registry::vanilla_fluids;
use steel_utils::{BlockPos, BlockStateId};

use super::weathering_block::{WeatherState, WeatheringCopper};
use crate::{
    behavior::{BlockBehavior, BlockPlaceContext},
    world::{LevelReader, ScheduledTickAccess, World},
};

/// Behavior for stair blocks.
#[block_behavior]
pub struct StairBlock {
    block: BlockRef,
}

impl StairBlock {
    const FACING: EnumProperty<Direction> = BlockStateProperties::HORIZONTAL_FACING;
    const HALF: EnumProperty<Half> = BlockStateProperties::HALF;
    const SHAPE: EnumProperty<StairsShape> = BlockStateProperties::STAIRS_SHAPE;

    /// Creates a new stair block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn update_stair_shape(
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> BlockStateId {
        state.set_value(&Self::SHAPE, Self::stairs_shape(state, world, pos))
    }

    fn stairs_shape(state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> StairsShape {
        let facing = state.get_value(&Self::FACING);
        let behind_state = world.get_block_state(facing.relative(pos));
        if Self::is_stairs(behind_state)
            && state.get_value(&Self::HALF) == behind_state.get_value(&Self::HALF)
        {
            let behind_facing = behind_state.get_value(&Self::FACING);
            if behind_facing.get_axis() != facing.get_axis()
                && Self::can_take_shape(state, world, pos, behind_facing.opposite())
            {
                if behind_facing == facing.rotate_y_counter_clockwise() {
                    return StairsShape::OuterLeft;
                }
                return StairsShape::OuterRight;
            }
        }

        let front_state = world.get_block_state(facing.opposite().relative(pos));
        if Self::is_stairs(front_state)
            && state.get_value(&Self::HALF) == front_state.get_value(&Self::HALF)
        {
            let front_facing = front_state.get_value(&Self::FACING);
            if front_facing.get_axis() != facing.get_axis()
                && Self::can_take_shape(state, world, pos, front_facing)
            {
                if front_facing == facing.rotate_y_counter_clockwise() {
                    return StairsShape::InnerLeft;
                }
                return StairsShape::InnerRight;
            }
        }

        StairsShape::Straight
    }

    fn can_take_shape(
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        neighbor: Direction,
    ) -> bool {
        let neighbor_state = world.get_block_state(neighbor.relative(pos));
        !Self::is_stairs(neighbor_state)
            || neighbor_state.get_value(&Self::FACING) != state.get_value(&Self::FACING)
            || neighbor_state.get_value(&Self::HALF) != state.get_value(&Self::HALF)
    }

    fn is_stairs(state: BlockStateId) -> bool {
        state.try_get_value(&Self::SHAPE).is_some()
    }
}

impl BlockBehavior for StairBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let half = if context.clicked_face != Direction::Down
            && (context.clicked_face == Direction::Up
                || context.click_location.y - f64::from(context.relative_pos.y()) <= 0.5)
        {
            Half::Bottom
        } else {
            Half::Top
        };

        let state = self
            .block
            .default_state()
            .set_value(&Self::FACING, context.horizontal_direction)
            .set_value(&Self::HALF, half)
            .set_value(
                &BlockStateProperties::WATERLOGGED,
                context.is_water_source(),
            );
        Some(Self::update_stair_shape(
            state,
            context.world.as_ref(),
            context.relative_pos,
        ))
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
        if state.get_value(&BlockStateProperties::WATERLOGGED) {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }

        if direction.is_horizontal() {
            Self::update_stair_shape(state, world, pos)
        } else {
            state
        }
    }
}

/// Weathering copper stairs share the stair shape rules and add copper aging.
#[block_behavior]
pub struct WeatheringCopperStairBlock {
    stair: StairBlock,
    #[json_arg(r#enum = "WeatherState", json = "weather_state")]
    weathering: WeatheringCopper,
}

impl WeatheringCopperStairBlock {
    /// Creates a new weathering copper stair block behavior.
    #[must_use]
    pub const fn new(block: BlockRef, weather_state: WeatherState) -> Self {
        Self {
            stair: StairBlock::new(block),
            weathering: WeatheringCopper::new(weather_state),
        }
    }
}

impl BlockBehavior for WeatheringCopperStairBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.stair.get_state_for_placement(context)
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
        self.stair
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        self.weathering.is_randomly_ticking()
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.weathering.change_over_time(state, world, pos);
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};
    use steel_utils::BlockPos;

    use crate::test_support::TestLevel;

    use super::*;

    #[test]
    fn stair_update_shape_recomputes_shape_from_neighbors() {
        init_test_registry();
        let behavior = StairBlock::new(&vanilla_blocks::DARK_OAK_STAIRS);
        let state = vanilla_blocks::DARK_OAK_STAIRS
            .default_state()
            .set_value(&StairBlock::FACING, Direction::West)
            .set_value(&StairBlock::HALF, Half::Top)
            .set_value(&StairBlock::SHAPE, StairsShape::OuterRight)
            .set_value(&BlockStateProperties::WATERLOGGED, true);
        let level = TestLevel::default();

        let updated = behavior.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::West,
            Direction::West.relative(BlockPos::ZERO),
            vanilla_blocks::AIR.default_state(),
        );

        assert_eq!(updated.get_value(&StairBlock::SHAPE), StairsShape::Straight);
        assert!(updated.get_value(&BlockStateProperties::WATERLOGGED));
    }
}
