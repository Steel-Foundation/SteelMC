use steel_macros::block_behavior;
use steel_registry::blocks::{
    BlockRef, block_state_ext::BlockStateExt, properties::BlockStateProperties, shapes::VoxelShape,
};
use steel_utils::{BlockLocalAabb, BlockPos, BlockStateId};

use crate::behavior::{BlockBehavior, BlockCollisionContext, BlockPlaceContext};
use crate::world::LevelReader;

const SHAPE_STABLE_BOXES: &[BlockLocalAabb] = &[
    BlockLocalAabb::new(0.0, 0.875, 0.0, 1.0, 1.0, 1.0),
    BlockLocalAabb::new(0.0, 0.0, 0.0, 0.125, 1.0, 0.125),
    BlockLocalAabb::new(0.875, 0.0, 0.0, 1.0, 1.0, 0.125),
    BlockLocalAabb::new(0.0, 0.0, 0.875, 0.125, 1.0, 1.0),
    BlockLocalAabb::new(0.875, 0.0, 0.875, 1.0, 1.0, 1.0),
];
const SHAPE_UNSTABLE_BOTTOM_BOXES: &[BlockLocalAabb] =
    &[BlockLocalAabb::new(0.0, 0.0, 0.0, 1.0, 0.125, 1.0)];
const SHAPE_BELOW_BLOCK_BOXES: &[BlockLocalAabb] =
    &[BlockLocalAabb::new(0.0, -1.0, 0.0, 1.0, 0.0, 1.0)];

const SHAPE_STABLE: VoxelShape = VoxelShape::from_boxes(SHAPE_STABLE_BOXES);
const SHAPE_UNSTABLE_BOTTOM: VoxelShape = VoxelShape::from_boxes(SHAPE_UNSTABLE_BOTTOM_BOXES);
const SHAPE_BELOW_BLOCK: VoxelShape = VoxelShape::from_boxes(SHAPE_BELOW_BLOCK_BOXES);

/// Vanilla scaffolding collision-shape behavior.
///
/// TODO: Add vanilla placement, stability distance updates, falling conversion, and waterlogging.
#[block_behavior]
pub struct ScaffoldingBlock {
    block: BlockRef,
}

impl ScaffoldingBlock {
    /// Creates a scaffolding block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for ScaffoldingBlock {
    // TODO: Mirror vanilla scaffolding placement here, including WATERLOGGED,
    // STABILITY_DISTANCE, BOTTOM, on_place, and update_shape tick scheduling.
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn get_collision_shape(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        pos: BlockPos,
        context: BlockCollisionContext,
    ) -> VoxelShape {
        if context.is_placement() {
            return VoxelShape::EMPTY;
        }

        if context.is_above(VoxelShape::FULL_BLOCK, pos, true) && !context.is_descending() {
            return SHAPE_STABLE;
        }

        let distance = state.get_value(&BlockStateProperties::STABILITY_DISTANCE);
        let bottom = state.get_value(&BlockStateProperties::BOTTOM);
        if distance != 0 && bottom && context.is_above(SHAPE_BELOW_BLOCK, pos, true) {
            SHAPE_UNSTABLE_BOTTOM
        } else {
            VoxelShape::EMPTY
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::{test_support, vanilla_blocks};

    use crate::test_support::TestLevel;

    fn scaffolding_state(distance: u8, bottom: bool) -> BlockStateId {
        vanilla_blocks::SCAFFOLDING
            .default_state()
            .set_value(&BlockStateProperties::STABILITY_DISTANCE, distance)
            .set_value(&BlockStateProperties::BOTTOM, bottom)
    }

    fn collision_shape(state: BlockStateId, context: BlockCollisionContext) -> VoxelShape {
        let behavior = ScaffoldingBlock::new(&vanilla_blocks::SCAFFOLDING);
        let level = TestLevel::default().with_min_y(0);
        behavior.get_collision_shape(state, &level, BlockPos::new(0, 64, 0), context)
    }

    #[test]
    fn placement_context_has_no_scaffolding_collision() {
        test_support::init_test_registry();

        let shape = collision_shape(
            scaffolding_state(0, false),
            BlockCollisionContext::pre_move(65.0, false),
        );

        assert_eq!(shape, VoxelShape::EMPTY);
    }

    #[test]
    fn entity_above_scaffolding_collides_with_stable_shape() {
        test_support::init_test_registry();

        let shape = collision_shape(
            scaffolding_state(0, false),
            BlockCollisionContext::entity(65.0, false),
        );

        assert_eq!(shape, SHAPE_STABLE);
    }

    #[test]
    fn descending_entity_only_collides_with_unstable_bottom_shape() {
        test_support::init_test_registry();

        let shape = collision_shape(
            scaffolding_state(1, true),
            BlockCollisionContext::entity(64.5, true),
        );

        assert_eq!(shape, SHAPE_UNSTABLE_BOTTOM);
    }

    #[test]
    fn non_bottom_descending_scaffolding_has_empty_collision() {
        test_support::init_test_registry();

        let shape = collision_shape(
            scaffolding_state(1, false),
            BlockCollisionContext::entity(64.5, true),
        );

        assert_eq!(shape, VoxelShape::EMPTY);
    }
}
