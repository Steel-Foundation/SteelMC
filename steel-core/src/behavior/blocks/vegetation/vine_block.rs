use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
use steel_registry::vanilla_blocks;
use steel_utils::axis::Axis;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess};

use super::{BlockRef, can_attach_to_multiface};

/// Vanilla `VineBlock` survival and neighbor shape updates.
// TODO: Implement random tick spread.
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

    fn has_faces(state: BlockStateId) -> bool {
        VINE_FACE_DIRECTIONS
            .into_iter()
            .any(|direction| state.get_value(get_property_for_face(direction)))
    }

    fn can_support_at_face(
        &self,
        world: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
    ) -> bool {
        if direction == Direction::Down {
            return false;
        }

        if can_attach_to_multiface(world, pos.relative(direction), direction) {
            return true;
        }

        if direction.get_axis() == Axis::Y {
            return false;
        }

        let property = get_property_for_face(direction);
        let above = world.get_block_state(pos.above());
        above.get_block() == self.block && above.get_value(property)
    }

    fn updated_state(
        &self,
        mut state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> BlockStateId {
        let above_pos = pos.above();
        if state.get_value(&BlockStateProperties::UP) {
            state = state.set_value(
                &BlockStateProperties::UP,
                can_attach_to_multiface(world, above_pos, Direction::Down),
            );
        }

        let mut above_state: Option<BlockStateId> = None;
        for direction in Direction::HORIZONTAL {
            let property = get_property_for_face(direction);
            if !state.get_value(property) {
                continue;
            }

            let mut can_support = self.can_support_at_face(world, pos, direction);
            if !can_support {
                let above = *above_state.get_or_insert_with(|| world.get_block_state(above_pos));
                can_support = above.get_block() == self.block && above.get_value(property);
            }

            state = state.set_value(property, can_support);
        }

        state
    }
}

const VINE_FACE_DIRECTIONS: [Direction; 5] = [
    Direction::Up,
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

fn get_property_for_face(direction: Direction) -> &'static BoolProperty {
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
        Self::has_faces(self.updated_state(state, world, pos))
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if direction == Direction::Down {
            return state;
        }

        let updated = self.updated_state(state, world, pos);
        if Self::has_faces(updated) {
            updated
        } else {
            if Self::has_faces(state) {
                return vanilla_blocks::AIR.default_state();
            }
            state
        }
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let clicked_state = context.world.get_block_state(context.get_clicked_pos());
        let clicked_vine = clicked_state.get_block() == self.block;
        let result = if clicked_vine {
            clicked_state
        } else {
            self.block.default_state()
        };

        for direction in context.get_nearest_looking_directions() {
            if direction != Direction::Down {
                let face = get_property_for_face(direction);
                let face_occupied = clicked_vine && clicked_state.get_value(face);
                if !face_occupied
                    && self.can_support_at_face(context.world, context.get_clicked_pos(), direction)
                {
                    return Some(result.set_value(face, true));
                }
            }
        }
        if clicked_vine {
            return Some(result);
        }
        None
    }
}
