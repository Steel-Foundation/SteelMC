use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, DoubleBlockHalf};
use steel_registry::fluid::{FluidRef, FluidState};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{vanilla_blocks, vanilla_fluids};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::bonemealable::Bonemealable;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::{BlockRef, water_source_fluid_state};

/// Behavior for seagrass blocks.
#[block_behavior]
pub struct SeagrassBlock {
    block: BlockRef,
}

impl SeagrassBlock {
    /// Creates a new seagrass block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for SeagrassBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let updated = if self.can_survive(state, world, pos) {
            state
        } else {
            vanilla_blocks::AIR.default_state()
        };

        if !updated.is_air() {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }

        updated
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below = world.get_block_state(below_pos);
        below.is_face_sturdy_at(below_pos, Direction::Up)
            && !below
                .get_block()
                .has_tag(&BlockTag::CANNOT_SUPPORT_SEAGRASS)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.block.default_state();
        (context.is_water_source() && self.can_survive(state, context.world, context.relative_pos))
            .then_some(state)
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

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Bonemealable for SeagrassBlock {
    fn is_valid_bonemeal_target(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        world.get_block_state(pos.above()).get_block() == &vanilla_blocks::WATER
    }

    fn perform_bonemeal(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn rand::Rng,
        pos: BlockPos,
    ) {
        let lower_state = vanilla_blocks::TALL_SEAGRASS.default_state().set_value(
            &BlockStateProperties::DOUBLE_BLOCK_HALF,
            DoubleBlockHalf::Lower,
        );
        let upper_state = lower_state.set_value(
            &BlockStateProperties::DOUBLE_BLOCK_HALF,
            DoubleBlockHalf::Upper,
        );
        world.set_block(pos, lower_state, UpdateFlags::UPDATE_CLIENTS);
        world.set_block(pos.above(), upper_state, UpdateFlags::UPDATE_CLIENTS);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use steel_registry::fluid::FluidRef;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks, vanilla_fluids};

    use super::*;

    struct SingleSupportLevel {
        support: BlockStateId,
        above: BlockStateId,
        scheduled_water_tick: Cell<bool>,
    }

    impl SingleSupportLevel {
        fn new(support: BlockStateId) -> Self {
            Self {
                support,
                above: vanilla_blocks::AIR.default_state(),
                scheduled_water_tick: Cell::new(false),
            }
        }

        fn with_above(mut self, above: BlockStateId) -> Self {
            self.above = above;
            self
        }
    }

    impl LevelReader for SingleSupportLevel {
        fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
            if pos == BlockPos::ZERO.below() {
                self.support
            } else if pos == BlockPos::ZERO.above() {
                self.above
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

    impl ScheduledTickAccess for SingleSupportLevel {
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
            fluid: FluidRef,
            _delay: i32,
        ) -> bool {
            let is_water = fluid == &vanilla_fluids::WATER;
            self.scheduled_water_tick.set(is_water);
            is_water
        }
    }

    #[test]
    fn seagrass_update_shape_breaks_without_support() {
        init_test_registry();
        let behavior = SeagrassBlock::new(&vanilla_blocks::SEAGRASS);
        let level = SingleSupportLevel::new(vanilla_blocks::AIR.default_state());
        let state = vanilla_blocks::SEAGRASS.default_state();

        let updated = behavior.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::Down,
            BlockPos::ZERO.below(),
            vanilla_blocks::AIR.default_state(),
        );

        assert!(updated.is_air());
        assert!(!level.scheduled_water_tick.get());
    }

    #[test]
    fn seagrass_update_shape_schedules_water_when_it_survives() {
        init_test_registry();
        let behavior = SeagrassBlock::new(&vanilla_blocks::SEAGRASS);
        let level = SingleSupportLevel::new(vanilla_blocks::DIRT.default_state());
        let state = vanilla_blocks::SEAGRASS.default_state();

        let updated = behavior.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::Down,
            BlockPos::ZERO.below(),
            vanilla_blocks::DIRT.default_state(),
        );

        assert_eq!(updated, state);
        assert!(level.scheduled_water_tick.get());
    }

    #[test]
    fn seagrass_bonemeal_requires_water_block_above() {
        init_test_registry();
        let behavior = SeagrassBlock::new(&vanilla_blocks::SEAGRASS);
        let state = vanilla_blocks::SEAGRASS.default_state();
        let waterlogged_slab = vanilla_blocks::OAK_SLAB
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, true);

        let water_level = SingleSupportLevel::new(vanilla_blocks::DIRT.default_state())
            .with_above(vanilla_blocks::WATER.default_state());
        assert!(behavior.is_valid_bonemeal_target(state, &water_level, BlockPos::ZERO));

        let waterlogged_level = SingleSupportLevel::new(vanilla_blocks::DIRT.default_state())
            .with_above(waterlogged_slab);
        assert!(!behavior.is_valid_bonemeal_target(state, &waterlogged_level, BlockPos::ZERO));
    }
}
