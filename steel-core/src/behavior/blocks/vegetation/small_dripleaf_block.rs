use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, DoubleBlockHalf};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::{BlockBehavior, schedule_water_tick_if_waterlogged};
use crate::behavior::context::{BlockPlaceContext, PlacementSource};
use crate::fluid::{FluidStateExt, get_fluid_state_from_block};
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::{BlockRef, DoublePlantBlock};

/// Vanilla `SmallDripleafBlock` survival.
// TODO: Implement full vanilla behavior beyond can_survive.
#[block_behavior]
pub struct SmallDripleafBlock {
    block: BlockRef,
    double_plant: DoublePlantBlock,
}

impl SmallDripleafBlock {
    /// Creates a new small dripleaf block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            block,
            double_plant: DoublePlantBlock::new(block),
        }
    }
}

impl BlockBehavior for SmallDripleafBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);
        self.double_plant.update_shape_with_survival(
            self,
            state,
            world,
            pos,
            direction,
            neighbor_state,
        )
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        if state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) == DoubleBlockHalf::Upper {
            return self.double_plant.can_survive(state, world, pos);
        }

        let below_pos = pos.below();
        let below = world.get_block_state(below_pos);
        let fluid = get_fluid_state_from_block(world.get_block_state(pos));
        below
            .get_block()
            .has_tag(&BlockTag::SUPPORTS_SMALL_DRIPLEAF)
            || (fluid.is_source()
                && fluid.is_water()
                && below.get_block().has_tag(&BlockTag::SUPPORTS_VEGETATION))
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if context.place_pos().y() >= context.world.max_y_exclusive() - 1 {
            return None;
        }
        if !context
            .world
            .get_block_state(context.place_pos().above())
            .is_replaceable()
        {
            return None;
        }
        let state = self.block.default_state().set_value(
            &BlockStateProperties::HORIZONTAL_FACING,
            context.horizontal_direction().opposite(),
        );
        self.can_survive(state, context.world, context.place_pos())
            .then_some(state.set_value(
                &BlockStateProperties::WATERLOGGED,
                context.is_water_source(),
            ))
    }

    fn set_placed_by(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source: &PlacementSource<'_>,
    ) {
        let upper_pos = pos.above();
        let upper_state = DoublePlantBlock::copy_waterlogged_from(
            world,
            upper_pos,
            self.block
                .default_state()
                .set_value(
                    &BlockStateProperties::DOUBLE_BLOCK_HALF,
                    DoubleBlockHalf::Upper,
                )
                .set_value(
                    &BlockStateProperties::HORIZONTAL_FACING,
                    state.get_value(&BlockStateProperties::HORIZONTAL_FACING),
                ),
        );
        world.set_block(upper_pos, upper_state, UpdateFlags::UPDATE_ALL);
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::TestLevel;

    #[test]
    fn small_dripleaf_schedules_water_before_double_plant_survival() {
        init_test_registry();
        init_behaviors();
        let behavior = SmallDripleafBlock::new(&vanilla_blocks::SMALL_DRIPLEAF);
        let state = vanilla_blocks::SMALL_DRIPLEAF
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, true)
            .set_value(
                &BlockStateProperties::DOUBLE_BLOCK_HALF,
                DoubleBlockHalf::Lower,
            );
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
