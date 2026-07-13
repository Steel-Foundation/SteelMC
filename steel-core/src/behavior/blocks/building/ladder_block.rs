use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::world::{LevelReader, ScheduledTickAccess};
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty, EnumProperty};
use steel_registry::fluid::FluidState;
use steel_registry::{vanilla_blocks, vanilla_fluids};
use steel_utils::{BlockPos, BlockStateId, Direction};

/// Whether the ladder is waterlogged or not.
const WATERLOGGED: BoolProperty = BlockStateProperties::WATERLOGGED;

/// The direction the ladder is facing.
const FACING: EnumProperty<Direction> = BlockStateProperties::FACING;

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

        if direction == facing && !self.can_survive(state, world, pos) {
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
        if !context.replaces_clicked_block {
            let state = context
                .world
                .get_block_state(context.place_pos.relative(context.clicked_face.opposite()));
            if state.get_block() == self.block && state.get_value(&FACING) == context.clicked_face {
                return None;
            }
        }

        let mut state = self.block.default_state();

        for direction in context.get_nearest_looking_directions() {
            if !direction.is_horizontal() {
                continue;
            }

            state = state.set_value(&FACING, direction.opposite());
            if self.can_survive(state, context.world, context.place_pos) {
                return Some(state.set_value(&WATERLOGGED, context.is_water_source()));
            }
        }

        None
    }

    fn get_fluid_state(&self, state: BlockStateId) -> FluidState {
        if state.get_value(&BlockStateProperties::WATERLOGGED) {
            FluidState::new(&vanilla_fluids::WATER, 8, true)
        } else {
            FluidState::EMPTY
        }
    }

    // TODO: Implement the mirror and rotate functions
}

/// Returns whether a ladder can be placed on a particular face of a block located at a certain position.
fn can_attach_to(world: &dyn LevelReader, pos: BlockPos, direction: Direction) -> bool {
    let state = world.get_block_state(pos);
    state.is_face_sturdy_at(pos, direction)
}
