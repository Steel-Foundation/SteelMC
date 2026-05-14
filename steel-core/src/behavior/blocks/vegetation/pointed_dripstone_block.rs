use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::BlockRef;

/// Vanilla `PointedDripstoneBlock` survival.
///
/// Survival mirrors vanilla's `isValidPointedDripstonePlacement`: the block
/// opposite the tip direction must be face-sturdy on the face pointing toward
/// us, or be another pointed dripstone with the same `vertical_direction`.
// TODO: Implement thickness recalculation, scheduled-tick stalagmite breakage,
// trident projectile breakage, fluid transfer, and growth.
#[block_behavior]
pub struct PointedDripstoneBlock {
    block: BlockRef,
}

impl PointedDripstoneBlock {
    /// Creates a new pointed dripstone block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for PointedDripstoneBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let tip_direction = state.get_value(&BlockStateProperties::VERTICAL_DIRECTION);
        let behind_pos = pos.relative(tip_direction.opposite());
        let behind_state = world.get_block_state(behind_pos);

        if behind_state.is_face_sturdy(tip_direction) {
            return true;
        }

        // Behind is pointed dripstone with the same tip direction.
        behind_state.get_block() == &vanilla_blocks::POINTED_DRIPSTONE
            && behind_state.get_value(&BlockStateProperties::VERTICAL_DIRECTION) == tip_direction
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // TODO: Vanilla picks tip direction from clicked-face/looking direction
        // and computes thickness. Placeholder: default state if it survives.
        let state = self.block.default_state();
        self.can_survive(state, context.world, context.relative_pos)
            .then_some(state.set_value(
                &BlockStateProperties::WATERLOGGED,
                context.is_water_source(),
            ))
    }
}
