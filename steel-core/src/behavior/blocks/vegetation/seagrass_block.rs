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
use crate::world::{LevelAccessor, LevelReader, ScheduledTickAccess, World};

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
        (context.is_full_water() && self.can_survive(state, context.world, context.relative_pos))
            .then_some(state)
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
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use crate::test_support::TestLevel;

    use super::*;

    fn seagrass_level(support: BlockStateId, above: BlockStateId) -> TestLevel {
        TestLevel::default()
            .with_block(BlockPos::ZERO.below(), support)
            .with_block(BlockPos::ZERO.above(), above)
    }

    #[test]
    fn seagrass_update_shape_breaks_without_support() {
        init_test_registry();
        let behavior = SeagrassBlock::new(&vanilla_blocks::SEAGRASS);
        let level = seagrass_level(
            vanilla_blocks::AIR.default_state(),
            vanilla_blocks::AIR.default_state(),
        );
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
        assert!(!level.scheduled_water_tick());
    }

    #[test]
    fn seagrass_update_shape_schedules_water_when_it_survives() {
        init_test_registry();
        let behavior = SeagrassBlock::new(&vanilla_blocks::SEAGRASS);
        let level = seagrass_level(
            vanilla_blocks::DIRT.default_state(),
            vanilla_blocks::AIR.default_state(),
        );
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
        assert!(level.scheduled_water_tick());
    }

    #[test]
    fn seagrass_bonemeal_requires_water_block_above() {
        init_test_registry();
        let behavior = SeagrassBlock::new(&vanilla_blocks::SEAGRASS);
        let state = vanilla_blocks::SEAGRASS.default_state();
        let waterlogged_slab = vanilla_blocks::OAK_SLAB
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, true);

        let water_level = seagrass_level(
            vanilla_blocks::DIRT.default_state(),
            vanilla_blocks::WATER.default_state(),
        );
        assert!(behavior.is_valid_bonemeal_target(state, &water_level, BlockPos::ZERO));

        let waterlogged_level =
            seagrass_level(vanilla_blocks::DIRT.default_state(), waterlogged_slab);
        assert!(!behavior.is_valid_bonemeal_target(state, &waterlogged_level, BlockPos::ZERO));
    }
}
