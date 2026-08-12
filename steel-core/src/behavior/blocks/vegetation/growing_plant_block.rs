use steel_registry::blocks::{BlockRef, block_state_ext::BlockStateExt};
use steel_utils::{BlockPos, Direction};

use crate::world::LevelReader;

/// Vanilla `GrowingPlantBlock.canSurvive`.
///
/// The block opposite the growth direction must be the head, the body, or
/// face-sturdy on the face pointing toward us (i.e. `growth_direction`).
pub(crate) fn can_survive(
    world: &dyn LevelReader,
    pos: BlockPos,
    growth_direction: Direction,
    head: BlockRef,
    body: BlockRef,
) -> bool {
    let attached_pos = pos.relative(growth_direction.opposite());
    let attached_state = world.get_block_state(attached_pos);
    let attached_block = attached_state.get_block();
    attached_block == head
        || attached_block == body
        || world.is_face_sturdy(attached_state, attached_pos, growth_direction)
}
