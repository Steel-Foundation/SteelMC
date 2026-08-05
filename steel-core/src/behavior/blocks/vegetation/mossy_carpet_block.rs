use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, EnumProperty, WallSide};
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess};

use super::{BlockRef, can_attach_to_multiface};

/// Vanilla `MossyCarpetBlock` survival and side state updates.
// TODO: Implement spreading, bonemeal, and the rest of vanilla behavior.
#[block_behavior]
pub struct MossyCarpetBlock {
    block: BlockRef,
}

impl MossyCarpetBlock {
    pub(crate) const HORIZONTAL_DIRECTIONS: [Direction; 4] = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];

    /// Creates a new mossy-carpet block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Vanilla `MossyCarpetBlock.getPropertyForFace`.
    pub(crate) const fn wall_property(direction: Direction) -> EnumProperty<WallSide> {
        match direction {
            Direction::North => BlockStateProperties::NORTH_WALL,
            Direction::East => BlockStateProperties::EAST_WALL,
            Direction::South => BlockStateProperties::SOUTH_WALL,
            Direction::West => BlockStateProperties::WEST_WALL,
            Direction::Down | Direction::Up => {
                panic!("mossy carpet has no wall property for vertical direction")
            }
        }
    }

    /// Vanilla `MossyCarpetBlock.hasFaces`.
    pub(crate) fn has_faces(state: BlockStateId) -> bool {
        if state.get_value(&BlockStateProperties::BOTTOM) {
            return true;
        }

        for direction in Self::HORIZONTAL_DIRECTIONS {
            let property = Self::wall_property(direction);
            if state.get_value(&property) != WallSide::None {
                return true;
            }
        }

        false
    }

    /// Vanilla `MossyCarpetBlock.canSupportAtFace`.
    pub(crate) fn can_support_at_face(
        world: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
    ) -> bool {
        direction != Direction::Up
            && can_attach_to_multiface(world, pos.relative(direction), direction)
    }

    /// Vanilla `MossyCarpetBlock.getUpdatedState`.
    pub(crate) fn updated_state(
        world: &dyn LevelReader,
        mut state: BlockStateId,
        pos: BlockPos,
        create_sides: bool,
    ) -> BlockStateId {
        let create_sides = create_sides || state.get_value(&BlockStateProperties::BOTTOM);
        let mut above_state = None;
        let mut below_state = None;

        for direction in Self::HORIZONTAL_DIRECTIONS {
            let property = Self::wall_property(direction);
            let mut side = if Self::can_support_at_face(world, pos, direction) {
                if create_sides {
                    WallSide::Low
                } else {
                    state.get_value(&property)
                }
            } else {
                WallSide::None
            };

            if side == WallSide::Low {
                let above = *above_state.get_or_insert_with(|| world.get_block_state(pos.above()));
                if above.get_block() == &vanilla_blocks::PALE_MOSS_CARPET
                    && above.get_value(&property) != WallSide::None
                    && !above.get_value(&BlockStateProperties::BOTTOM)
                {
                    side = WallSide::Tall;
                }

                if !state.get_value(&BlockStateProperties::BOTTOM) {
                    let below =
                        *below_state.get_or_insert_with(|| world.get_block_state(pos.below()));
                    if below.get_block() == &vanilla_blocks::PALE_MOSS_CARPET
                        && below.get_value(&property) == WallSide::None
                    {
                        side = WallSide::None;
                    }
                }
            }

            state = state.set_value(&property, side);
        }

        state
    }
}

impl BlockBehavior for MossyCarpetBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if !self.can_survive(state, world, pos) {
            return vanilla_blocks::AIR.default_state();
        }

        let updated = Self::updated_state(world, state, pos, false);
        if Self::has_faces(updated) {
            updated
        } else {
            vanilla_blocks::AIR.default_state()
        }
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        if state.get_value(&BlockStateProperties::BOTTOM) {
            !world.get_block_state(pos.below()).is_air()
        } else {
            let below = world.get_block_state(pos.below());
            below.get_block() == &vanilla_blocks::PALE_MOSS_CARPET
                && below.get_value(&BlockStateProperties::BOTTOM)
        }
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = Self::updated_state(
            context.world,
            self.block.default_state(),
            context.place_pos(),
            true,
        );
        (self.can_survive(state, context.world, context.place_pos()) && Self::has_faces(state))
            .then_some(state)
    }
}
