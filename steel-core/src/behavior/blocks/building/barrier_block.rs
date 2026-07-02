use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::fluid::FluidRef;
use steel_registry::vanilla_fluids;
use steel_utils::types::GameType;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::player::Player;
use crate::world::ScheduledTickAccess;

/// Vanilla `BarrierBlock` liquid-container behavior.
#[block_behavior]
pub struct BarrierBlock {
    block: BlockRef,
}

impl BarrierBlock {
    /// Creates a new barrier block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for BarrierBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state().set_value(
            &BlockStateProperties::WATERLOGGED,
            context.is_water_source(),
        ))
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if state.get_value(&BlockStateProperties::WATERLOGGED) {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }

        state
    }

    fn can_place_liquid(&self, _state: BlockStateId, _fluid: FluidRef) -> bool {
        false
    }

    fn can_place_liquid_with_player(
        &self,
        state: BlockStateId,
        fluid: FluidRef,
        player: Option<&Player>,
    ) -> bool {
        player.is_some_and(|player| player.game_mode() == GameType::Creative)
            && state
                .try_get_value(&BlockStateProperties::WATERLOGGED)
                .is_some()
            && fluid == &vanilla_fluids::WATER
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::{BLOCK_BEHAVIORS, init_behaviors};
    use crate::test_support::TestLevel;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    #[test]
    fn registered_barrier_rejects_no_user_liquid_placement() {
        init_test_registry();
        init_behaviors();
        let behavior = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::BARRIER);
        let dry_barrier = vanilla_blocks::BARRIER
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, false);

        assert!(behavior.is_liquid_container(dry_barrier));
        assert!(!behavior.can_place_liquid(dry_barrier, &vanilla_fluids::WATER));
    }

    #[test]
    fn waterlogged_barrier_update_shape_schedules_water_tick() {
        init_test_registry();

        let behavior = BarrierBlock::new(&vanilla_blocks::BARRIER);
        let state = vanilla_blocks::BARRIER
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, true);
        let level = TestLevel::default();

        let updated = behavior.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::North,
            Direction::North.relative(BlockPos::ZERO),
            vanilla_blocks::AIR.default_state(),
        );

        assert_eq!(updated, state);
        assert!(level.scheduled_water_tick());
    }
}
