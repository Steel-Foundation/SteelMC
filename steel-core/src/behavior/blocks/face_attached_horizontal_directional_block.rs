//! Shared vanilla face-attached horizontal placement and support behavior.

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{
    AttachFace, BlockStateProperties, Direction, EnumProperty,
};
use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::axis::Axis;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{BLOCK_BEHAVIORS, BlockPlaceContext};
use crate::world::LevelReader;

const ATTACH_FACE: &EnumProperty<AttachFace> = &BlockStateProperties::ATTACH_FACE;
const HORIZONTAL_FACING: &EnumProperty<Direction> = &BlockStateProperties::HORIZONTAL_FACING;

/// Shared behavior inherited from vanilla's `FaceAttachedHorizontalDirectionalBlock`.
pub(in crate::behavior::blocks) struct FaceAttachedHorizontalDirectionalBlock {
    pub(in crate::behavior::blocks) block: BlockRef,
}

impl FaceAttachedHorizontalDirectionalBlock {
    #[must_use]
    pub(in crate::behavior::blocks) const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    pub(in crate::behavior::blocks) fn connected_direction(state: BlockStateId) -> Direction {
        match state.get_value(ATTACH_FACE) {
            AttachFace::Ceiling => Direction::Down,
            AttachFace::Floor => Direction::Up,
            AttachFace::Wall => state.get_value(HORIZONTAL_FACING),
        }
    }

    pub(in crate::behavior::blocks) fn can_attach(
        level: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
    ) -> bool {
        let support_pos = pos.relative(direction);
        level.is_face_sturdy(
            level.get_block_state(support_pos),
            support_pos,
            direction.opposite(),
        )
    }

    pub(in crate::behavior::blocks) fn can_survive(
        state: BlockStateId,
        level: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        Self::can_attach(level, pos, Self::connected_direction(state).opposite())
    }

    /// Vanilla's placement and neighbor updates call the virtual `state.canSurvive`
    /// rather than this class's own, and subclasses override it — `GrindstoneBlock`
    /// always survives. Dispatch through the block's behavior so those overrides
    /// are honored instead of forcing the attachment rule on every subclass.
    fn block_can_survive(state: BlockStateId, level: &dyn LevelReader, pos: BlockPos) -> bool {
        BLOCK_BEHAVIORS
            .get_behavior(state.get_block())
            .can_survive(state, level, pos)
    }

    pub(in crate::behavior::blocks) fn state_for_placement(
        &self,
        context: &BlockPlaceContext<'_>,
    ) -> Option<BlockStateId> {
        for direction in context.get_nearest_looking_directions() {
            let state = if direction.get_axis() == Axis::Y {
                self.block
                    .default_state()
                    .set_value(
                        ATTACH_FACE,
                        if direction == Direction::Up {
                            AttachFace::Ceiling
                        } else {
                            AttachFace::Floor
                        },
                    )
                    .set_value(HORIZONTAL_FACING, context.horizontal_direction())
            } else {
                self.block
                    .default_state()
                    .set_value(ATTACH_FACE, AttachFace::Wall)
                    .set_value(HORIZONTAL_FACING, direction.opposite())
            };

            if Self::block_can_survive(state, context.world.as_ref(), context.place_pos()) {
                return Some(state);
            }
        }
        None
    }

    pub(in crate::behavior::blocks) fn update_shape(
        state: BlockStateId,
        level: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
    ) -> BlockStateId {
        if Self::connected_direction(state).opposite() == direction
            && !Self::block_can_survive(state, level, pos)
        {
            REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR)
        } else {
            state
        }
    }
}
