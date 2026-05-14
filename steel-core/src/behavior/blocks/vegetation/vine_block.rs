use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, can_attach_to_multiface, default_surviving_state};

/// Vanilla `VineBlock` survival.
// TODO: Implement placement, random tick spread, and shape updates.
#[block_behavior]
pub struct VineBlock {
    block: BlockRef,
}

impl VineBlock {
    /// Creates a new vine block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

/// Vanilla `VineBlock.getPropertyForFace`. Only called with horizontal
/// directions from `can_survive`; `Down` is unreachable.
fn face_property(direction: Direction) -> &'static BoolProperty {
    match direction {
        Direction::Up => &BlockStateProperties::UP,
        Direction::North => &BlockStateProperties::NORTH,
        Direction::East => &BlockStateProperties::EAST,
        Direction::South => &BlockStateProperties::SOUTH,
        Direction::West => &BlockStateProperties::WEST,
        Direction::Down => unreachable!("vine has no DOWN face property"),
    }
}

impl BlockBehavior for VineBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        // Vanilla: hasFaces(getUpdatedState(state, level, pos)) > 0.
        //
        // getUpdatedState recomputes each face property:
        //   - UP: kept iff isAcceptableNeighbor(above, DOWN)
        //   - For each horizontal D set: kept iff canSupportAtFace(pos, D)
        //     OR (above is vine AND above.D is true).
        //
        // We short-circuit on the first surviving face.
        //
        // canSupportAtFace(pos, D) for horizontal D is:
        //   isAcceptableNeighbor(pos+D, D)
        //     OR (above is vine AND above.D is true)
        // — note vanilla's fallback in getUpdatedState duplicates the second
        // branch from canSupportAtFace, so we only need to check it once.
        let above_pos = pos.above();

        if state.get_value(&BlockStateProperties::UP)
            && can_attach_to_multiface(world, above_pos, Direction::Down)
        {
            return true;
        }

        let mut above_state: Option<BlockStateId> = None;
        for direction in Direction::HORIZONTAL {
            let property = face_property(direction);
            if !state.get_value(property) {
                continue;
            }

            if can_attach_to_multiface(world, pos.relative(direction), direction) {
                return true;
            }

            let above = *above_state.get_or_insert_with(|| world.get_block_state(above_pos));
            if above.get_block() == self.block && above.get_value(property) {
                return true;
            }
        }

        false
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // TODO: Vanilla picks a face based on nearest looking direction and
        // supports replacing an existing vine. Placeholder: default state if it
        // survives.
        default_surviving_state(self.block, self, context)
    }
}
