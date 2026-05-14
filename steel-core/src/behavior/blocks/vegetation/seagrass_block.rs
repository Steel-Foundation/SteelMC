use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::fluid::FluidState;
use steel_registry::vanilla_block_tags;
use steel_registry::{REGISTRY, TaggedRegistryExt};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

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
