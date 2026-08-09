use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, Direction, DoubleBlockHalf, EnumProperty,
};
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId, axis::Axis, types::UpdateFlags};

use crate::behavior::BlockStateBehaviorExt;
use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::Vegetation;
use crate::behavior::blocks::vegetation::vegetation_block::vegetation_can_survive;
use crate::behavior::context::{BlockPlaceContext, PlacementSource};
use crate::fluid::{FluidStateExt as _, get_fluid_state};
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::BlockRef;

/// Behavior for vanilla two-block-tall plants.
#[block_behavior]
pub struct DoublePlantBlock {
    pub(super) block: BlockRef,
}

const HALF: EnumProperty<DoubleBlockHalf> = BlockStateProperties::DOUBLE_BLOCK_HALF;
const WATERLOGGED: BoolProperty = BlockStateProperties::WATERLOGGED;

impl DoublePlantBlock {
    /// Creates a new double plant block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    pub(super) fn copy_waterlogged_from(
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockStateId {
        if state.try_get_value(&WATERLOGGED).is_some() {
            state.set_value(&WATERLOGGED, get_fluid_state(world, pos).is_water())
        } else {
            state
        }
    }

    /// Runs Vanilla `DoublePlantBlock.updateShape` while preserving virtual
    /// `canSurvive` dispatch for subclasses such as small dripleaf.
    pub(super) fn update_shape_with_survival(
        &self,
        survival_behavior: &dyn BlockBehavior,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let half = state.get_value(&HALF);
        let neighbor_is_matching_other_half =
            neighbor_state.get_block() == self.block && neighbor_state.get_value(&HALF) != half;

        if direction.get_axis() == Axis::Y
            && (half == DoubleBlockHalf::Lower) == (direction == Direction::Up)
            && !neighbor_is_matching_other_half
        {
            return vanilla_blocks::AIR.default_state();
        }

        if half == DoubleBlockHalf::Lower
            && direction == Direction::Down
            && !survival_behavior.can_survive(state, world, pos)
        {
            return vanilla_blocks::AIR.default_state();
        }

        state
    }
    pub(super) fn place_at(
        world: &Arc<World>,
        state: BlockStateId,
        lower_pos: BlockPos,
        update_type: UpdateFlags,
    ) {
        let upper_pos = lower_pos.above();
        world.set_block(
            lower_pos,
            Self::copy_waterlogged_from(
                world,
                lower_pos,
                state.set_value(&HALF, DoubleBlockHalf::Lower),
            ),
            update_type,
        );
        world.set_block(
            upper_pos,
            Self::copy_waterlogged_from(
                world,
                upper_pos,
                state.set_value(&HALF, DoubleBlockHalf::Upper),
            ),
            update_type,
        );
    }
}

impl Vegetation for DoublePlantBlock {}

impl BlockBehavior for DoublePlantBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.update_shape_with_survival(self, state, world, pos, direction, neighbor_state)
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        if state.get_value(&HALF) == DoubleBlockHalf::Upper {
            let state_below = world.get_block_state(pos.below());
            state_below.get_block() == state.get_block()
                && state_below.get_value(&HALF) == DoubleBlockHalf::Lower
        } else {
            vegetation_can_survive(self, state, world, pos)
        }
    }

    fn set_placed_by(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source: &PlacementSource<'_>,
    ) {
        let upper_pos = pos.above();
        let upper_state = Self::copy_waterlogged_from(
            world,
            upper_pos,
            self.block
                .default_state()
                .set_value(&HALF, DoubleBlockHalf::Upper),
        );
        world.set_block(upper_pos, upper_state, UpdateFlags::UPDATE_ALL);
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if context.place_pos().y() >= context.world.max_y_exclusive() - 1 {
            return None;
        }
        if !context
            .world
            .get_block_state(context.place_pos().above())
            .can_be_replaced(context)
        {
            return None;
        }
        Some(self.block.default_state())
    }
}
