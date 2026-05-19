use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::fluid::FluidState;
use steel_registry::vanilla_block_tags;
use steel_registry::{REGISTRY, TaggedRegistryExt, vanilla_blocks, vanilla_fluids};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess};

use super::{BlockRef, water_source_fluid_state};

/// Vanilla `SeagrassBlock` survival and fluid state.
// TODO: Implement full vanilla behavior beyond can_survive and get_fluid_state.
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
        let below = world.get_block_state(pos.below());
        below.is_face_sturdy(Direction::Up)
            && !REGISTRY.blocks.is_in_tag(
                below.get_block(),
                &vanilla_block_tags::CANNOT_SUPPORT_SEAGRASS_TAG,
            )
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.block.default_state();
        (context.is_water_source() && self.can_survive(state, context.world, context.relative_pos))
            .then_some(state)
    }

    fn get_fluid_state(&self, _state: BlockStateId) -> FluidState {
        water_source_fluid_state()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use steel_registry::fluid::FluidRef;
    use steel_registry::{REGISTRY, Registry, vanilla_blocks, vanilla_fluids};

    use super::*;

    struct SingleSupportLevel {
        support: BlockStateId,
        scheduled_water_tick: Cell<bool>,
    }

    impl SingleSupportLevel {
        fn new(support: BlockStateId) -> Self {
            Self {
                support,
                scheduled_water_tick: Cell::new(false),
            }
        }
    }

    impl LevelReader for SingleSupportLevel {
        fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
            if pos == BlockPos::ZERO.below() {
                self.support
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

    fn init_registry() {
        let mut registry = Registry::new_vanilla();
        registry.freeze();
        let _ = REGISTRY.init(registry);
    }

    #[test]
    fn seagrass_update_shape_breaks_without_support() {
        init_registry();
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
        init_registry();
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
}
