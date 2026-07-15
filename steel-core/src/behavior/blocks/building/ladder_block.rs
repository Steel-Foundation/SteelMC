use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::world::{LevelReader, ScheduledTickAccess};
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty, EnumProperty};
use steel_registry::{vanilla_blocks, vanilla_fluids};
use steel_utils::{BlockPos, BlockStateId, Direction};

/// Whether the ladder is waterlogged or not.
const WATERLOGGED: BoolProperty = BlockStateProperties::WATERLOGGED;

/// The direction the ladder is facing.
const FACING: EnumProperty<Direction> = BlockStateProperties::HORIZONTAL_FACING;

/// Behavior for ladders.
#[block_behavior]
pub struct LadderBlock {
    block: BlockRef,
}

impl LadderBlock {
    /// Creates a ladder block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for LadderBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let facing: Direction = state.get_value(&FACING);

        if direction == facing.opposite() && !self.can_survive(state, world, pos) {
            return vanilla_blocks::AIR.default_state();
        }

        if state.get_value(&WATERLOGGED) {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }

        state
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let direction = state.get_value(&FACING);
        can_attach_to(world, pos.relative(direction.opposite()), direction)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if !context.replaces_clicked_block() {
            let state = context.world.get_block_state(
                context
                    .place_pos()
                    .relative(context.clicked_face().opposite()),
            );
            if state.get_block() == self.block && state.get_value(&FACING) == context.clicked_face()
            {
                return None;
            }
        }

        let mut state = self.block.default_state();

        for direction in context.get_nearest_looking_directions() {
            if !direction.is_horizontal() {
                continue;
            }

            state = state.set_value(&FACING, direction.opposite());
            if self.can_survive(state, context.world, context.place_pos()) {
                return Some(state.set_value(&WATERLOGGED, context.is_water_source()));
            }
        }

        None
    }
    // TODO: Implement the mirror and rotate functions
}

/// Returns whether a ladder can be placed on a particular face of a block located at a certain position.
fn can_attach_to(world: &dyn LevelReader, pos: BlockPos, direction: Direction) -> bool {
    let state = world.get_block_state(pos);
    state.is_face_sturdy_at(pos, direction)
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::fluid::FluidRef;
    use steel_registry::test_support::init_test_registry;

    struct EmptyLevel;

    impl LevelReader for EmptyLevel {
        fn get_block_state(&self, _pos: BlockPos) -> BlockStateId {
            vanilla_blocks::AIR.default_state()
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

    impl ScheduledTickAccess for EmptyLevel {
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
    fn support_neighbor_update_breaks_unsupported_ladder() {
        init_test_registry();
        let behavior = LadderBlock::new(&vanilla_blocks::LADDER);
        let state = vanilla_blocks::LADDER
            .default_state()
            .set_value(&FACING, Direction::East);

        let updated = behavior.update_shape(
            state,
            &EmptyLevel,
            BlockPos::ZERO,
            Direction::West,
            BlockPos::ZERO.relative(Direction::West),
            vanilla_blocks::AIR.default_state(),
        );

        assert_eq!(updated, vanilla_blocks::AIR.default_state());
    }

    #[test]
    fn waterlogged_ladder_contains_non_falling_source_water() {
        init_test_registry();
        let behavior = LadderBlock::new(&vanilla_blocks::LADDER);
        let state = vanilla_blocks::LADDER
            .default_state()
            .set_value(&WATERLOGGED, true);

        let fluid = behavior.get_fluid_state(state);

        assert_eq!(fluid.fluid_id, &vanilla_fluids::WATER);
        assert!(fluid.is_source());
        assert!(!fluid.falling);
    }
}
