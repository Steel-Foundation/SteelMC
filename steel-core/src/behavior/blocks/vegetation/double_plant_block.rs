use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, DoubleBlockHalf};
<<<<<<< HEAD
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId, math::Axis, types::UpdateFlags};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::Vegetation;
use crate::behavior::blocks::vegetation::default_surviving_state;
use crate::behavior::blocks::vegetation::vegetation_block::double_plant_can_survive;
use crate::behavior::context::{BlockPlaceContext, InventoryAccess};
=======
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId, math::Axis, types::UpdateFlags};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::Vegetation;
use crate::behavior::blocks::vegetation::default_surviving_state;
use crate::behavior::blocks::vegetation::vegetation_block::double_plant_can_survive;
use crate::behavior::context::BlockPlaceContext;
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
use crate::fluid::{FluidStateExt as _, get_fluid_state};
use crate::player::Player;
use crate::world::{LevelReader, ScheduledTickAccess, World};

<<<<<<< HEAD
<<<<<<< HEAD
use super::BlockRef;

/// Behavior for vanilla two-block-tall plants.
=======
use super::{BlockRef, default_surviving_state, survives_on_tag};

/// Vanilla `DoublePlantBlock` lower/upper-half survival.
// TODO: Implement full vanilla behavior beyond can_survive.
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
=======
use super::BlockRef;

/// Behavior for vanilla two-block-tall plants.
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
#[block_behavior]
pub struct DoublePlantBlock {
    block: BlockRef,
}

impl DoublePlantBlock {
<<<<<<< HEAD
<<<<<<< HEAD
    /// Creates a new double plant block behavior.
=======
    /// Creates a new double-plant block behavior.
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
=======
    /// Creates a new double plant block behavior.
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    pub(super) fn copy_waterlogged_from(
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockStateId {
        if state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .is_some()
        {
            state.set_value(
                &BlockStateProperties::WATERLOGGED,
                get_fluid_state(world, pos).is_water(),
            )
        } else {
            state
        }
    }
}

<<<<<<< HEAD
<<<<<<< HEAD
impl Vegetation for DoublePlantBlock {}

=======
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
=======
impl Vegetation for DoublePlantBlock {}

>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
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
        let half = state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF);
        let neighbor_is_matching_other_half = neighbor_state.get_block() == self.block
            && neighbor_state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) != half;

        if direction.get_axis() == Axis::Y
            && (half == DoubleBlockHalf::Lower) == (direction == Direction::Up)
            && !neighbor_is_matching_other_half
        {
            return vanilla_blocks::AIR.default_state();
        }

        if half == DoubleBlockHalf::Lower
            && direction == Direction::Down
            && !self.can_survive(state, world, pos)
        {
            return vanilla_blocks::AIR.default_state();
        }

        state
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
<<<<<<< HEAD
<<<<<<< HEAD
        double_plant_can_survive(self, state, world, pos)
=======
        if state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) == DoubleBlockHalf::Upper {
            let below = world.get_block_state(pos.below());
            return below.get_block() == self.block
                && below.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF)
                    == DoubleBlockHalf::Lower;
        }

        survives_on_tag(world, pos, &vanilla_block_tags::SUPPORTS_VEGETATION_TAG)
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
=======
        double_plant_can_survive(self, state, world, pos)
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
    }

    fn set_placed_by(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _player: Option<&Player>,
<<<<<<< HEAD
        _inv: &InventoryAccess,
=======
        _item_stack: &ItemStack,
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
    ) {
        let upper_pos = pos.above();
        let upper_state = Self::copy_waterlogged_from(
            world,
            upper_pos,
            self.block.default_state().set_value(
                &BlockStateProperties::DOUBLE_BLOCK_HALF,
                DoubleBlockHalf::Upper,
            ),
        );
        world.set_block(upper_pos, upper_state, UpdateFlags::UPDATE_ALL);
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if context.relative_pos.y() >= context.world.max_y_exclusive() - 1 {
            return None;
        }
        if !context
            .world
            .get_block_state(context.relative_pos.above())
            .is_replaceable()
        {
            return None;
        }
        default_surviving_state(self.block, self, context)
    }
}
