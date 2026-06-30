use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, DoubleBlockHalf};
use steel_registry::fluid::{FluidRef, FluidState, FluidStateExt as _};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_items;
use steel_utils::axis::Axis;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::fluid::get_fluid_state_from_block;
use crate::world::{LevelAccessor, LevelReader, ScheduledTickAccess};

use super::{BlockRef, water_source_fluid_state};

/// Behavior for tall seagrass blocks.
#[block_behavior]
pub struct TallSeagrassBlock {
    block: BlockRef,
}

impl TallSeagrassBlock {
    /// Creates a new tall seagrass block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for TallSeagrassBlock {
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

        if self.can_survive(state, world, pos) {
            state
        } else {
            vanilla_blocks::AIR.default_state()
        }
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        if state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) == DoubleBlockHalf::Upper {
            let below = world.get_block_state(pos.below());
            return below.get_block() == self.block
                && below.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF)
                    == DoubleBlockHalf::Lower;
        }

        let below_pos = pos.below();
        let below = world.get_block_state(below_pos);
        let current = world.get_block_state(pos);
        let fluid = if current.get_block() == self.block {
            water_source_fluid_state()
        } else {
            get_fluid_state_from_block(current)
        };
        below.is_face_sturdy_at(below_pos, Direction::Up)
            && !below
                .get_block()
                .has_tag(&BlockTag::CANNOT_SUPPORT_SEAGRASS)
            && fluid.is_water()
            && fluid.is_full()
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if context.relative_pos.y() >= context.world.max_y_exclusive() - 1 {
            return None;
        }
        if !context.is_full_water() {
            return None;
        }

        let above_fluid =
            get_fluid_state_from_block(context.world.get_block_state(context.relative_pos.above()));
        if !above_fluid.is_water() || !above_fluid.is_full() {
            return None;
        }

        let state = self.block.default_state().set_value(
            &BlockStateProperties::DOUBLE_BLOCK_HALF,
            DoubleBlockHalf::Lower,
        );
        self.can_survive(state, context.world, context.relative_pos)
            .then_some(state)
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        Some(ItemStack::new(&vanilla_items::ITEMS.seagrass))
    }

    fn get_fluid_state(&self, _state: BlockStateId) -> FluidState {
        water_source_fluid_state()
    }

    fn is_liquid_container(&self, _state: BlockStateId) -> bool {
        true
    }

    fn place_liquid(
        &self,
        _level: &dyn LevelAccessor,
        _pos: BlockPos,
        _state: BlockStateId,
        _fluid_state: FluidState,
    ) -> bool {
        false
    }

    fn can_place_liquid(&self, _state: BlockStateId, _fluid: FluidRef) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use crate::behavior::init_behaviors;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use crate::test_support::TestLevel;

    use super::*;

    fn tall_seagrass_level(below: BlockStateId, current: BlockStateId) -> TestLevel {
        TestLevel::default()
            .with_block(BlockPos::ZERO.below(), below)
            .with_block(BlockPos::ZERO, current)
    }

    #[test]
    fn tall_seagrass_lower_breaks_when_upper_half_is_missing() {
        init_test_registry();
        let behavior = TallSeagrassBlock::new(&vanilla_blocks::TALL_SEAGRASS);
        let lower = vanilla_blocks::TALL_SEAGRASS.default_state().set_value(
            &BlockStateProperties::DOUBLE_BLOCK_HALF,
            DoubleBlockHalf::Lower,
        );
        let level = tall_seagrass_level(vanilla_blocks::DIRT.default_state(), lower);

        let updated = behavior.update_shape(
            lower,
            &level,
            BlockPos::ZERO,
            Direction::Up,
            BlockPos::ZERO.above(),
            vanilla_blocks::WATER.default_state(),
        );

        assert!(updated.is_air());
    }

    #[test]
    fn tall_seagrass_upper_breaks_when_lower_half_is_missing() {
        init_test_registry();
        let behavior = TallSeagrassBlock::new(&vanilla_blocks::TALL_SEAGRASS);
        let upper = vanilla_blocks::TALL_SEAGRASS.default_state().set_value(
            &BlockStateProperties::DOUBLE_BLOCK_HALF,
            DoubleBlockHalf::Upper,
        );
        let level = tall_seagrass_level(vanilla_blocks::AIR.default_state(), upper);

        let updated = behavior.update_shape(
            upper,
            &level,
            BlockPos::ZERO,
            Direction::Down,
            BlockPos::ZERO.below(),
            vanilla_blocks::AIR.default_state(),
        );

        assert!(updated.is_air());
    }

    #[test]
    fn tall_seagrass_lower_survives_in_falling_full_water() {
        init_test_registry();
        init_behaviors();
        let behavior = TallSeagrassBlock::new(&vanilla_blocks::TALL_SEAGRASS);
        let lower = vanilla_blocks::TALL_SEAGRASS.default_state().set_value(
            &BlockStateProperties::DOUBLE_BLOCK_HALF,
            DoubleBlockHalf::Lower,
        );
        let falling_full_water = vanilla_blocks::WATER
            .default_state()
            .set_value(&BlockStateProperties::LEVEL, 8);
        let level = tall_seagrass_level(vanilla_blocks::DIRT.default_state(), falling_full_water);

        assert!(behavior.can_survive(lower, &level, BlockPos::ZERO));
    }
}
