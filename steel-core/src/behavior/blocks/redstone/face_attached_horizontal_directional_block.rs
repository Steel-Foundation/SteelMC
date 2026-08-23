//! Shared vanilla face-attached horizontal placement and support behavior.

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{
    AttachFace, BlockStateProperties, Direction, EnumProperty,
};
use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::axis::Axis;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::BlockPlaceContext;
use crate::world::LevelReader;

const ATTACH_FACE: &EnumProperty<AttachFace> = &BlockStateProperties::ATTACH_FACE;
const HORIZONTAL_FACING: &EnumProperty<Direction> = &BlockStateProperties::HORIZONTAL_FACING;

/// Shared behavior inherited from vanilla's `FaceAttachedHorizontalDirectionalBlock`.
pub(crate) struct FaceAttachedHorizontalDirectionalBlock {
    pub(crate) block: BlockRef,
}

impl FaceAttachedHorizontalDirectionalBlock {
    #[must_use]
    pub(crate) const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    pub(crate) fn connected_direction(state: BlockStateId) -> Direction {
        match state.get_value(ATTACH_FACE) {
            AttachFace::Ceiling => Direction::Down,
            AttachFace::Floor => Direction::Up,
            AttachFace::Wall => state.get_value(HORIZONTAL_FACING),
        }
    }

    pub(crate) fn can_attach(level: &dyn LevelReader, pos: BlockPos, direction: Direction) -> bool {
        let support_pos = pos.relative(direction);
        level.is_face_sturdy(
            level.get_block_state(support_pos),
            support_pos,
            direction.opposite(),
        )
    }

    pub(crate) fn can_survive(state: BlockStateId, level: &dyn LevelReader, pos: BlockPos) -> bool {
        Self::can_attach(level, pos, Self::connected_direction(state).opposite())
    }

    pub(crate) fn state_for_placement(
        &self,
        context: &BlockPlaceContext<'_>,
    ) -> Option<BlockStateId> {
        self.state_for_placement_checking_support(context, true)
    }

    /// Placement with an optional support check. Grindstones skip it.
    pub(crate) fn state_for_placement_checking_support(
        &self,
        context: &BlockPlaceContext<'_>,
        require_support: bool,
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

            if !require_support
                || Self::can_survive(state, context.world.as_ref(), context.place_pos())
            {
                return Some(state);
            }
        }
        None
    }

    pub(crate) fn update_shape(
        state: BlockStateId,
        level: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
    ) -> BlockStateId {
        if Self::connected_direction(state).opposite() == direction
            && !Self::can_survive(state, level, pos)
        {
            REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR)
        } else {
            state
        }
    }
}
