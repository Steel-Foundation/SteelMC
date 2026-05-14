use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, DoubleBlockHalf};
use steel_registry::fluid::{FluidState, FluidStateExt as _};
use steel_registry::vanilla_block_tags;
use steel_registry::vanilla_blocks;
use steel_registry::{REGISTRY, TaggedRegistryExt};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::fluid::get_fluid_state_from_block;
use crate::world::LevelReader;

use super::{BlockRef, water_source_fluid_state};

/// Vanilla `TallSeagrassBlock` survival and fluid state.
// TODO: Implement full vanilla behavior beyond can_survive and get_fluid_state.
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
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        if state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) == DoubleBlockHalf::Upper {
            let below = world.get_block_state(pos.below());
            return below.get_block() == &vanilla_blocks::TALL_SEAGRASS
                && below.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF)
                    == DoubleBlockHalf::Lower;
        }

        let below = world.get_block_state(pos.below());
        let current = world.get_block_state(pos);
        let fluid = if current.get_block() == &vanilla_blocks::TALL_SEAGRASS {
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
        let state = self.block.default_state().set_value(
            &BlockStateProperties::DOUBLE_BLOCK_HALF,
            DoubleBlockHalf::Lower,
        );
        if !context.is_water_source() {
            return None;
        }

        let above = context.world.get_block_state(context.relative_pos.above());
        if above.get_block() != &vanilla_blocks::WATER {
            return None;
        }

        self.can_survive(state, context.world, context.relative_pos)
            .then_some(state)
    }

    fn get_fluid_state(&self, _state: BlockStateId) -> FluidState {
        water_source_fluid_state()
    }
}
