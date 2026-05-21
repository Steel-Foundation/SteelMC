use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, DoubleBlockHalf};
use steel_registry::fluid::{FluidRef, FluidState, FluidStateExt as _};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_tags;
use steel_registry::vanilla_blocks;
use steel_registry::{REGISTRY, TaggedRegistryExt, vanilla_items};
use steel_utils::math::Axis;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::fluid::get_fluid_state_from_block;
use crate::world::{LevelReader, ScheduledTickAccess, World};

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

        let below = world.get_block_state(pos.below());
        let current = world.get_block_state(pos);
        let fluid = if current.get_block() == self.block {
            water_source_fluid_state()
        } else {
            get_fluid_state_from_block(current)
        };
        below.is_face_sturdy(Direction::Up)
            && !REGISTRY.blocks.is_in_tag(
                below.get_block(),
                &vanilla_block_tags::CANNOT_SUPPORT_SEAGRASS_TAG,
            )
            && fluid.is_water()
            && fluid.is_source()
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if context.relative_pos.y() >= context.world.max_y_exclusive() - 1 {
            return None;
        }
        if !context.is_water_source() {
            return None;
        }

        let above_fluid =
            get_fluid_state_from_block(context.world.get_block_state(context.relative_pos.above()));
        if !above_fluid.is_water() || !above_fluid.is_source() {
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

    fn place_liquid(
        &self,
        _world: &Arc<World>,
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
    use steel_registry::fluid::FluidRef;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use super::*;

    struct TallSeagrassLevel {
        below: BlockStateId,
        current: BlockStateId,
    }

    impl TallSeagrassLevel {
        const fn new(below: BlockStateId, current: BlockStateId) -> Self {
            Self { below, current }
        }
    }

    impl LevelReader for TallSeagrassLevel {
        fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
            if pos == BlockPos::ZERO.below() {
                self.below
            } else if pos == BlockPos::ZERO {
                self.current
            } else {
                vanilla_blocks::AIR.default_state()
            }
        }

        fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
            0
        }

        fn min_y(&self) -> i32 {
            -64
        }

        fn height(&self) -> i32 {
            384
        }
    }

    impl ScheduledTickAccess for TallSeagrassLevel {
        fn fluid_tick_delay(&self, _fluid: FluidRef) -> i32 {
            5
        }

        fn schedule_block_tick_default(
            &self,
            _pos: BlockPos,
            _block: BlockRef,
            _delay: i32,
        ) -> bool {
            true
        }

        fn schedule_fluid_tick_default(
            &self,
            _pos: BlockPos,
            _fluid: FluidRef,
            _delay: i32,
        ) -> bool {
            true
        }
    }

    #[test]
    fn tall_seagrass_lower_breaks_when_upper_half_is_missing() {
        init_test_registry();
        let behavior = TallSeagrassBlock::new(&vanilla_blocks::TALL_SEAGRASS);
        let lower = vanilla_blocks::TALL_SEAGRASS.default_state().set_value(
            &BlockStateProperties::DOUBLE_BLOCK_HALF,
            DoubleBlockHalf::Lower,
        );
        let level = TallSeagrassLevel::new(vanilla_blocks::DIRT.default_state(), lower);

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
        let level = TallSeagrassLevel::new(vanilla_blocks::AIR.default_state(), upper);

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
}
