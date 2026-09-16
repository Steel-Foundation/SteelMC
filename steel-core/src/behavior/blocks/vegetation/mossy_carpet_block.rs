use std::sync::Arc;

use crate::behavior::PlacementSource;
use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::MultifaceBlock;
use crate::behavior::blocks::vegetation::bonemealable::Bonemealable;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess, World};
use rand::Rng;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, Direction, EnumProperty, WallSide,
};
use steel_registry::vanilla_blocks;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

/// Vanilla `MossyCarpetBlock` survival and side state updates.
#[block_behavior]
pub struct MossyCarpetBlock {
    block: BlockRef,
}

const BASE: &BoolProperty = &BlockStateProperties::BOTTOM;
const EAST_WALL: &EnumProperty<WallSide> = &BlockStateProperties::EAST_WALL;
const NORTH_WALL: &EnumProperty<WallSide> = &BlockStateProperties::NORTH_WALL;
const SOUTH_WALL: &EnumProperty<WallSide> = &BlockStateProperties::SOUTH_WALL;
const WEST_WALL: &EnumProperty<WallSide> = &BlockStateProperties::WEST_WALL;

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
    pub(crate) const fn wall_property(direction: Direction) -> &'static EnumProperty<WallSide> {
        match direction {
            Direction::North => NORTH_WALL,
            Direction::East => EAST_WALL,
            Direction::South => SOUTH_WALL,
            Direction::West => WEST_WALL,
            Direction::Down | Direction::Up => {
                panic!("mossy carpet has no wall property for vertical direction")
            }
        }
    }

    /// Vanilla `MossyCarpetBlock.hasFaces`.
    pub(crate) fn has_faces(state: BlockStateId) -> bool {
        if state.get_value(BASE) {
            return true;
        }

        for direction in Self::HORIZONTAL_DIRECTIONS {
            let property = Self::wall_property(direction);
            if state.get_value(property) != WallSide::None {
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
        direction != Direction::Up && MultifaceBlock::can_attach_to(world, pos, direction)
    }

    /// Vanilla `MossyCarpetBlock.getUpdatedState`.
    pub(crate) fn updated_state(
        mut state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        create_sides: bool,
    ) -> BlockStateId {
        let create_sides = create_sides || state.get_value(BASE);
        let mut above_state = None;
        let mut below_state = None;

        for direction in Self::HORIZONTAL_DIRECTIONS {
            let property = Self::wall_property(direction);
            let mut side = if Self::can_support_at_face(world, pos, direction) {
                if create_sides {
                    WallSide::Low
                } else {
                    state.get_value(property)
                }
            } else {
                WallSide::None
            };

            if side == WallSide::Low {
                let above = *above_state.get_or_insert_with(|| world.get_block_state(pos.above()));
                if above.get_block() == &vanilla_blocks::PALE_MOSS_CARPET
                    && above.get_value(property) != WallSide::None
                    && !above.get_value(BASE)
                {
                    side = WallSide::Tall;
                }

                if !state.get_value(BASE) {
                    let below =
                        *below_state.get_or_insert_with(|| world.get_block_state(pos.below()));
                    if below.get_block() == &vanilla_blocks::PALE_MOSS_CARPET
                        && below.get_value(property) == WallSide::None
                    {
                        side = WallSide::None;
                    }
                }
            }

            state = state.set_value(property, side);
        }

        state
    }

    fn create_topper_with_side_chance(
        world: &dyn LevelReader,
        pos: BlockPos,
        side_survival_test: bool,
    ) -> BlockStateId {
        let above = pos.above();
        let above_previous_state = world.get_block_state(above);
        let is_mossy_carpet_above =
            above_previous_state.get_block() == &vanilla_blocks::PALE_MOSS_CARPET;
        if (!is_mossy_carpet_above || !above_previous_state.get_value(BASE))
            && (is_mossy_carpet_above || above_previous_state.is_replaceable())
        {
            let no_carpet_base_state = &vanilla_blocks::PALE_MOSS_CARPET
                .default_state()
                .set_value(BASE, false);
            let mut above_state =
                Self::updated_state(*no_carpet_base_state, world, pos.above(), true);

            for dir in Direction::HORIZONTAL {
                let property = Self::wall_property(dir);
                if above_state.get_value(property) != WallSide::None && !side_survival_test {
                    above_state = above_state.set_value(property, WallSide::None);
                }
            }

            if Self::has_faces(above_state) && above_state != above_previous_state {
                return above_state;
            }
            return vanilla_blocks::AIR.default_state();
        }
        vanilla_blocks::AIR.default_state()
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

        let updated = Self::updated_state(state, world, pos, false);
        if Self::has_faces(updated) {
            updated
        } else {
            vanilla_blocks::AIR.default_state()
        }
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        if state.get_value(BASE) {
            !world.get_block_state(pos.below()).is_air()
        } else {
            let below = world.get_block_state(pos.below());
            below.get_block() == self.block && below.get_value(BASE)
        }
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = Self::updated_state(
            self.block.default_state(),
            context.world,
            context.place_pos(),
            true,
        );
        (self.can_survive(state, context.world, context.place_pos()) && Self::has_faces(state))
            .then_some(state)
    }

    fn set_placed_by(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source: &PlacementSource<'_>,
    ) {
        let random = rand::random::<bool>();
        let topper = Self::create_topper_with_side_chance(world, pos, random);
        if !topper.is_air() {
            world.set_block(pos.above(), topper, UpdateFlags::UPDATE_ALL);
        }
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Bonemealable for MossyCarpetBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        state.get_value(BASE) && !Self::create_topper_with_side_chance(world, pos, true).is_air()
    }

    fn perform_bonemeal(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        let topper = Self::create_topper_with_side_chance(world, pos, true);
        if !topper.is_air() {
            world.set_block(pos.above(), topper, UpdateFlags::UPDATE_ALL);
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::TestLevel;

    #[test]
    fn updated_state_walls_up_against_the_supporting_block() {
        init_vanilla_registry();
        init_behaviors();

        let pos = BlockPos::new(0, 64, 0);
        let level =
            TestLevel::default().with_block(pos.north(), vanilla_blocks::STONE.default_state());

        let updated = MossyCarpetBlock::updated_state(
            vanilla_blocks::PALE_MOSS_CARPET.default_state(),
            &level,
            pos,
            true,
        );

        assert_eq!(updated.get_value(NORTH_WALL), WallSide::Low);
        assert_eq!(updated.get_value(SOUTH_WALL), WallSide::None);
    }
}
