use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
use steel_registry::{vanilla_blocks, vanilla_game_rules};
use steel_utils::axis::Axis;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::{BlockBehavior, default_can_be_replaced};
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::{BlockRef, can_attach_to_multiface};

/// Vanilla `VineBlock` survival and neighbor shape updates.
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
        Self::count_faces(state) > 0
    }

    fn count_faces(state: BlockStateId) -> usize {
        VINE_FACE_DIRECTIONS
            .into_iter()
            .filter(|direction| state.get_value(get_property_for_face(*direction)))
            .count()
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
    fn is_acceptable_neighbour(
        level: &dyn LevelReader,
        neighbour_pos: BlockPos,
        direction_to_neighbour: Direction,
    ) -> bool {
        can_attach_to_multiface(level, neighbour_pos, direction_to_neighbour)
    }
    fn can_spread(&self, world: &Arc<World>, pos: BlockPos) -> bool {
        let mut max = 5;

        for x in (pos.x() - 4)..=(pos.x() + 4) {
            for y in (pos.y() - 1)..=(pos.y() + 1) {
                for z in (pos.z() - 4)..=(pos.z() + 4) {
                    let block_pos = BlockPos::new(x, y, z);

                    if world.get_block_state(block_pos).get_block() == self.block {
                        max -= 1;

                        if max <= 0 {
                            return false;
                        }
                    }
                }
            }
        }

        true
    }

    fn copy_random_faces(from: BlockStateId, to: BlockStateId) -> BlockStateId {
        let mut result = to;
        for direction in Direction::HORIZONTAL {
            if rand::random_bool(0.5) {
                let property_for_face = get_property_for_face(direction);
                if from.get_value(property_for_face) {
                    result = result.set_value(property_for_face, true);
                }
            }
        }

        result
    }
    fn has_horizontal_connection(state: BlockStateId) -> bool {
        for dir in Direction::HORIZONTAL {
            let property = get_property_for_face(dir);
            if state.get_value(property) {
                return true;
            }
        }
        false
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
                Self::is_acceptable_neighbour(world, above_pos, Direction::Down),
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
    fn try_spread_horizontal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        test_direction: Direction,
    ) {
        if !self.can_spread(world, pos) {
            return;
        }

        let test_pos = pos.relative(test_direction);
        let edge_state = world.get_block_state(test_pos);

        if edge_state.is_air() {
            let cw = test_direction.rotate_y_clockwise();
            let cocw = test_direction.rotate_y_counter_clockwise();
            let cw_property = get_property_for_face(cw);
            let cocw_property = get_property_for_face(cocw);
            let cw_has_connecting_face = state.get_value(cw_property);
            let cocw_has_connecting_face = state.get_value(cocw_property);
            let cw_test_pos = test_pos.relative(cw);
            let cocw_test_pos = test_pos.relative(cocw);

            if cw_has_connecting_face && Self::is_acceptable_neighbour(world, cw_test_pos, cw) {
                world.set_block(
                    test_pos,
                    self.block.default_state().set_value(cw_property, true),
                    UpdateFlags::UPDATE_CLIENTS,
                );
            } else if cocw_has_connecting_face
                && Self::is_acceptable_neighbour(world, cocw_test_pos, cocw)
            {
                world.set_block(
                    test_pos,
                    self.block.default_state().set_value(cocw_property, true),
                    UpdateFlags::UPDATE_CLIENTS,
                );
            } else {
                let opposite = test_direction.opposite();
                if cw_has_connecting_face
                    && world.get_block_state(cw_test_pos).is_air()
                    && Self::is_acceptable_neighbour(world, pos.relative(cw), opposite)
                {
                    world.set_block(
                        cw_test_pos,
                        self.block
                            .default_state()
                            .set_value(get_property_for_face(opposite), true),
                        UpdateFlags::UPDATE_CLIENTS,
                    );
                } else if cocw_has_connecting_face
                    && world.get_block_state(cocw_test_pos).is_air()
                    && Self::is_acceptable_neighbour(world, pos.relative(cocw), opposite)
                {
                    world.set_block(
                        cocw_test_pos,
                        self.block
                            .default_state()
                            .set_value(get_property_for_face(opposite), true),
                        UpdateFlags::UPDATE_CLIENTS,
                    );
                } else if rand::random_range(0.0..1.0) < 0.05
                    && Self::is_acceptable_neighbour(world, test_pos.above(), Direction::Up)
                {
                    world.set_block(
                        test_pos,
                        self.block
                            .default_state()
                            .set_value(get_property_for_face(Direction::Up), true),
                        UpdateFlags::UPDATE_CLIENTS,
                    );
                }
            }
        } else if Self::is_acceptable_neighbour(world, test_pos, test_direction) {
            world.set_block(
                pos,
                state.set_value(get_property_for_face(test_direction), true),
                UpdateFlags::UPDATE_CLIENTS,
            );
        }
    }

    fn try_spread_vertical(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        test_direction: Direction,
    ) {
        let above_pos = pos.above();

        if test_direction == Direction::Up && pos.y() < world.get_max_y() {
            if self.can_support_at_face(world, pos, test_direction) {
                world.set_block(
                    pos,
                    state.set_value(get_property_for_face(Direction::Up), true),
                    UpdateFlags::UPDATE_CLIENTS,
                );
                return;
            }
            if world.get_block_state(above_pos).is_air() {
                if !self.can_spread(world, pos) {
                    return;
                }
                let mut above_state = state;
                for direction in Direction::HORIZONTAL {
                    if rand::random_bool(0.5)
                        || !Self::is_acceptable_neighbour(
                            world,
                            above_pos.relative(direction),
                            direction,
                        )
                    {
                        above_state =
                            above_state.set_value(get_property_for_face(direction), false);
                    }
                }
                if Self::has_horizontal_connection(above_state) {
                    world.set_block(above_pos, above_state, UpdateFlags::UPDATE_CLIENTS);
                }
                return;
            }
        }

        if pos.y() > world.min_y() {
            let below_pos = pos.below();
            let below_state = world.get_block_state(below_pos);
            if below_state.is_air() || below_state.get_block() == self.block {
                let before = if below_state.is_air() {
                    self.block.default_state()
                } else {
                    below_state
                };
                let after = Self::copy_random_faces(state, before);
                if before != after && Self::has_horizontal_connection(after) {
                    world.set_block(below_pos, after, UpdateFlags::UPDATE_CLIENTS);
                }
            }
        }
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
            vanilla_blocks::AIR.default_state()
        }
    }
    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        true
    }
    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !world.get_game_rule(&vanilla_game_rules::SPREAD_VINES) {
            return;
        }
        if rand::random_range(0..4) != 0 {
            return;
        }

        let test_direction = Direction::random();

        if test_direction.axis().is_horizontal()
            && !state.get_value(get_property_for_face(test_direction))
        {
            self.try_spread_horizontal(state, world, pos, test_direction);
        } else {
            self.try_spread_vertical(state, world, pos, test_direction);
        }
    }

    fn can_be_replaced(&self, state: BlockStateId, context: &BlockPlaceContext<'_>) -> bool {
        let clicked_state = context.world.get_block_state(context.place_pos());
        if clicked_state.get_block() == self.block {
            Self::count_faces(clicked_state) < VINE_FACE_DIRECTIONS.len()
        } else {
            default_can_be_replaced(state, context)
        }
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let clicked_pos = context.place_pos();
        let clicked_state = context.world.get_block_state(clicked_pos);
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
                if !face_occupied && self.can_support_at_face(context.world, clicked_pos, direction)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestLevel;
    use steel_registry::test_support::init_test_registry;

    #[test]
    fn face_count_matches_vanilla_replacement_limit() {
        init_test_registry();

        let mut state = vanilla_blocks::VINE.default_state();
        assert_eq!(VineBlock::count_faces(state), 0);

        for (expected, direction) in VINE_FACE_DIRECTIONS.into_iter().enumerate() {
            state = state.set_value(get_property_for_face(direction), true);
            assert_eq!(VineBlock::count_faces(state), expected + 1);
        }
    }

    #[test]
    fn shape_update_removes_faceless_vine() {
        init_test_registry();

        let vine = VineBlock::new(&vanilla_blocks::VINE);
        let state = vanilla_blocks::VINE.default_state();
        let level = TestLevel::default();

        assert_eq!(
            vine.update_shape(
                state,
                &level,
                BlockPos::ZERO,
                Direction::North,
                BlockPos::ZERO.north(),
                vanilla_blocks::AIR.default_state(),
            ),
            vanilla_blocks::AIR.default_state()
        );
    }
}
