use crate::{
    behavior::{BlockBehavior, BlockPlaceContext},
    world::{LevelReader, ScheduledTickAccess},
};
use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, BoolProperty, EnumProperty},
    },
    vanilla_blocks, vanilla_fluids,
};
use steel_utils::{BlockPos, BlockStateId, Direction};

/// Behavior for vanilla amethyst clusters blocks.
#[block_behavior]
pub struct AmethystClusterBlock {
    block: BlockRef,
}

const FACING: &EnumProperty<Direction> = &BlockStateProperties::FACING;
const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;

impl AmethystClusterBlock {
    /// Creates a new cluster block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for AmethystClusterBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self
            .block
            .default_state()
            .set_value(WATERLOGGED, context.is_water_source())
            .set_value(FACING, context.clicked_face());
        self.can_survive(state, context.world, context.place_pos())
            .then_some(state)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if state.get_value(WATERLOGGED) {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }

        if direction == state.get_value(FACING).opposite() && !self.can_survive(state, world, pos) {
            vanilla_blocks::AIR.default_state()
        } else {
            state
        }
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let direction = state.get_value(FACING);
        let adjacent = pos.relative(direction.opposite());
        world
            .get_block_state(adjacent)
            .is_face_sturdy_at(adjacent, direction)
    }
    // TODO: OnProjectile hit from AmethystBlock inheritance
    // TODO: Mirror and Rotate functions
}
