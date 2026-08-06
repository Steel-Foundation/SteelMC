use std::sync::Arc;

use rand::Rng;
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{
    BlockStateProperties, Direction, DoubleBlockHalf, EnumProperty,
};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::{BlockBehavior, schedule_water_tick_if_waterlogged};
use crate::behavior::blocks::BigDripleafBlock;
use crate::behavior::blocks::vegetation::bonemealable::Bonemealable;
use crate::behavior::context::{BlockPlaceContext, PlacementSource};
use crate::fluid::{FluidStateExt, get_fluid_state_from_block};
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::{BlockRef, DoublePlantBlock};

/// Vanilla `SmallDripleafBlock` survival.
#[block_behavior]
pub struct SmallDripleafBlock {
    block: BlockRef,
    base: DoublePlantBlock,
}
const HALF: EnumProperty<DoubleBlockHalf> = BlockStateProperties::DOUBLE_BLOCK_HALF;
const FACING: EnumProperty<Direction> = BlockStateProperties::HORIZONTAL_FACING;

impl SmallDripleafBlock {
    /// Creates a new small dripleaf block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            block,
            base: DoublePlantBlock::new(block),
        }
    }

    fn may_place_on(state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let fluid = get_fluid_state_from_block(world.get_block_state(pos.above()));
        state
            .get_block()
            .has_tag(&BlockTag::SUPPORTS_SMALL_DRIPLEAF)
            || (fluid.is_full() && fluid.is_water())
                && state.get_block().has_tag(&BlockTag::SUPPORTS_VEGETATION)
    }
}

impl BlockBehavior for SmallDripleafBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);
        self.base
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        if state.get_value(&HALF) == DoubleBlockHalf::Upper {
            return self.base.can_survive(state, world, pos);
        }

        let below_pos = pos.below();
        let below_state = world.get_block_state(below_pos);
        Self::may_place_on(below_state, world, below_pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self
            .base
            .get_state_for_placement(context)?
            .set_value(&FACING, context.horizontal_direction().opposite());
        Some(DoublePlantBlock::copy_waterlogged_from(
            context.world,
            context.place_pos(),
            state,
        ))
    }

    fn set_placed_by(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source: &PlacementSource<'_>,
    ) {
        let above_pos = pos.above();
        let block_state = DoublePlantBlock::copy_waterlogged_from(
            world,
            above_pos,
            self.block
                .default_state()
                .set_value(&HALF, DoubleBlockHalf::Upper)
                .set_value(&FACING, state.get_value(&FACING)),
        );
        world.set_block(above_pos, block_state, UpdateFlags::UPDATE_ALL);
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}
impl Bonemealable for SmallDripleafBlock {
    fn is_valid_bonemeal_target(
        &self,
        _state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
    ) -> bool {
        true
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        if state.get_value(&HALF) == DoubleBlockHalf::Lower {
            let above = pos.above();
            world.set_block(
                above,
                world
                    .get_block_state(above)
                    .get_fluid_state()
                    .create_legacy_block(),
                UpdateFlags::UPDATE_CLIENTS | UpdateFlags::UPDATE_KNOWN_SHAPE,
            );
            BigDripleafBlock::place_with_random_height(world, rng, pos, state.get_value(&FACING));
            return;
        }
        let below_pos = pos.below();
        Self::perform_bonemeal(
            self,
            world.get_block_state(below_pos),
            world,
            rng,
            below_pos,
        );
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::blocks::properties::BoolProperty;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::TestLevel;

    const WATERLOGGED: BoolProperty = BlockStateProperties::WATERLOGGED;

    #[test]
    fn small_dripleaf_schedules_water_before_double_plant_survival() {
        init_test_registry();
        init_behaviors();
        let behavior = SmallDripleafBlock::new(&vanilla_blocks::SMALL_DRIPLEAF);
        let state = vanilla_blocks::SMALL_DRIPLEAF
            .default_state()
            .set_value(&WATERLOGGED, true)
            .set_value(&HALF, DoubleBlockHalf::Lower);
        let level = TestLevel::default();

        assert!(
            behavior
                .update_shape(
                    state,
                    &level,
                    BlockPos::ZERO,
                    Direction::Down,
                    BlockPos::ZERO.below(),
                    vanilla_blocks::AIR.default_state(),
                )
                .is_air()
        );
        assert!(level.scheduled_water_tick());
    }
}
