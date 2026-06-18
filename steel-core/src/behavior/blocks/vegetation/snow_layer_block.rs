use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::blocks::shapes;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::BlockRef;

/// Vanilla `SnowLayerBlock` survival.
///
/// 1. If below is in `cannot_support_snow_layer`, false.
/// 2. If below is in `support_override_snow_layer`, true.
/// 3. Otherwise: below's collision shape has a full UP face, or below is snow
///    with `LAYERS = 8`.
// TODO: Implement melting, layering on placement, and entity step damage.
#[block_behavior]
pub struct SnowLayerBlock {
    block: BlockRef,
}

impl SnowLayerBlock {
    /// Creates a new snow layer block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for SnowLayerBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below = world.get_block_state(pos.below());
        let below_block = below.get_block();

        if below_block.has_tag(&BlockTag::CANNOT_SUPPORT_SNOW_LAYER) {
            return false;
        }

        if below_block.has_tag(&BlockTag::SUPPORT_OVERRIDE_SNOW_LAYER) {
            return true;
        }

        if shapes::is_offset_face_full(below.get_collision_shape_at(pos.below()), Direction::Up) {
            return true;
        }

        // Below is another snow layer fully filled (LAYERS == 8).
        below_block == self.block && below.get_value(&BlockStateProperties::LAYERS) == 8
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.block.default_state();
        self.can_survive(state, context.world, context.relative_pos)
            .then_some(state)
    }
}
