use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::ai::path::PathComputationType;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::BlockRef;

const HORIZONTAL_DIRECTIONS: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

/// Vanilla `ChorusPlantBlock` connection and survival behavior.
#[block_behavior]
pub struct ChorusPlantBlock {
    block: BlockRef,
}

const DOWN: &BoolProperty = &BlockStateProperties::DOWN;
const EAST: &BoolProperty = &BlockStateProperties::EAST;
const NORTH: &BoolProperty = &BlockStateProperties::NORTH;
const SOUTH: &BoolProperty = &BlockStateProperties::SOUTH;
const UP: &BoolProperty = &BlockStateProperties::UP;
const WEST: &BoolProperty = &BlockStateProperties::WEST;

impl ChorusPlantBlock {
    /// Creates a new chorus plant block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    #[must_use]
    pub(crate) fn state_with_connections(
        world: &dyn LevelReader,
        pos: BlockPos,
        mut state: BlockStateId,
    ) -> BlockStateId {
        let down = world.get_block_state(pos.below());
        let up = world.get_block_state(pos.above());
        let north = world.get_block_state(pos.north());
        let east = world.get_block_state(pos.east());
        let south = world.get_block_state(pos.south());
        let west = world.get_block_state(pos.west());
        let block = state.get_block();

        state = state.set_value(
            DOWN,
            down.get_block() == block
                || down.get_block() == &vanilla_blocks::CHORUS_FLOWER
                || down.get_block().has_tag(&BlockTag::SUPPORTS_CHORUS_PLANT),
        );
        state = state.set_value(
            UP,
            up.get_block() == block || up.get_block() == &vanilla_blocks::CHORUS_FLOWER,
        );
        state = state.set_value(
            NORTH,
            north.get_block() == block || north.get_block() == &vanilla_blocks::CHORUS_FLOWER,
        );
        state = state.set_value(
            EAST,
            east.get_block() == block || east.get_block() == &vanilla_blocks::CHORUS_FLOWER,
        );
        state = state.set_value(
            SOUTH,
            south.get_block() == block || south.get_block() == &vanilla_blocks::CHORUS_FLOWER,
        );
        state.set_value(
            WEST,
            west.get_block() == block || west.get_block() == &vanilla_blocks::CHORUS_FLOWER,
        )
    }

    const fn property_for_direction(direction: Direction) -> &'static BoolProperty {
        match direction {
            Direction::Down => DOWN,
            Direction::Up => UP,
            Direction::North => NORTH,
            Direction::South => SOUTH,
            Direction::West => WEST,
            Direction::East => EAST,
        }
    }
}

impl BlockBehavior for ChorusPlantBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_state = world.get_block_state(pos.below());
        let block_above_or_below =
            !world.get_block_state(pos.above()).is_air() && !below_state.is_air();

        for direction in HORIZONTAL_DIRECTIONS {
            let neighbor_pos = pos.relative(direction);
            let neighbor_state = world.get_block_state(neighbor_pos);
            if neighbor_state.get_block() == self.block {
                if block_above_or_below {
                    return false;
                }

                let below = world.get_block_state(neighbor_pos.below());
                if below.get_block() == self.block
                    || below.get_block().has_tag(&BlockTag::SUPPORTS_CHORUS_PLANT)
                {
                    return true;
                }
            }
        }

        below_state.get_block() == self.block
            || below_state
                .get_block()
                .has_tag(&BlockTag::SUPPORTS_CHORUS_PLANT)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(Self::state_with_connections(
            context.world.as_ref(),
            context.place_pos(),
            self.block.default_state(),
        ))
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if !self.can_survive(state, world, pos) {
            world.schedule_block_tick_default(pos, self.block, 1);
            return state;
        }

        let connects = neighbor_state.get_block() == self.block
            || neighbor_state.get_block() == &vanilla_blocks::CHORUS_FLOWER
            || direction == Direction::Down
                && neighbor_state
                    .get_block()
                    .has_tag(&BlockTag::SUPPORTS_CHORUS_PLANT);
        state.set_value(Self::property_for_direction(direction), connects)
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !self.can_survive(state, world, pos) {
            world.destroy_block(pos, true);
        }
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use crate::test_support::TestLevel;

    use super::*;

    #[test]
    fn update_shape_tracks_vertical_connections() {
        init_vanilla_registry();
        let behavior = ChorusPlantBlock::new(&vanilla_blocks::CHORUS_PLANT);
        let pos = BlockPos::ZERO;
        let state = vanilla_blocks::CHORUS_PLANT.default_state();
        let level =
            TestLevel::default().with_block(pos.below(), vanilla_blocks::END_STONE.default_state());

        let connected = behavior.update_shape(
            state,
            &level,
            pos,
            Direction::Up,
            pos.above(),
            vanilla_blocks::CHORUS_FLOWER.default_state(),
        );
        assert!(connected.get_value(UP));

        let disconnected = behavior.update_shape(
            connected,
            &level,
            pos,
            Direction::Up,
            pos.above(),
            vanilla_blocks::AIR.default_state(),
        );
        assert!(!disconnected.get_value(UP));
    }

    #[test]
    fn update_shape_schedules_tick_when_unsupported() {
        init_vanilla_registry();
        let behavior = ChorusPlantBlock::new(&vanilla_blocks::CHORUS_PLANT);
        let level = TestLevel::default();
        let pos = BlockPos::ZERO;
        let state = vanilla_blocks::CHORUS_PLANT.default_state();

        assert_eq!(
            behavior.update_shape(
                state,
                &level,
                pos,
                Direction::Down,
                pos.below(),
                vanilla_blocks::AIR.default_state(),
            ),
            state
        );

        let scheduled = level.scheduled_block_ticks.borrow();
        assert_eq!(scheduled.len(), 1);
        assert_eq!(scheduled[0].pos, pos);
        assert_eq!(scheduled[0].block, &vanilla_blocks::CHORUS_PLANT);
        assert_eq!(scheduled[0].delay, 1);
    }

    #[test]
    fn is_never_pathfindable() {
        init_vanilla_registry();
        let behavior = ChorusPlantBlock::new(&vanilla_blocks::CHORUS_PLANT);
        let state = vanilla_blocks::CHORUS_PLANT.default_state();

        assert!(!behavior.is_pathfindable(state, PathComputationType::Land));
        assert!(!behavior.is_pathfindable(state, PathComputationType::Water));
        assert!(!behavior.is_pathfindable(state, PathComputationType::Air));
    }
}
