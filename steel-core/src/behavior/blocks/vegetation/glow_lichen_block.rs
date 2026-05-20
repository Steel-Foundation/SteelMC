use steel_macros::block_behavior;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state, multiface_can_survive};

/// Vanilla `GlowLichenBlock` survival.
///
/// Inherits `canSurvive` from `MultifaceBlock`. Subclass-specific spread and
/// bonemeal behavior is left as a TODO.
// TODO: Implement spread, bonemeal, and rotation/mirror overrides.
#[block_behavior]
pub struct GlowLichenBlock {
    block: BlockRef,
}

impl GlowLichenBlock {
    /// Creates a new glow lichen block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for GlowLichenBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        multiface_can_survive(state, world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // TODO: Vanilla picks a face from nearestLookingDirections; placeholder
        // accepts the default state if it survives at the click position.
        default_surviving_state(self.block, self, context)
    }
}
