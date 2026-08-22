use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty, Direction};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::MultifaceBlock;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess};

use super::BlockRef;

/// Vanilla huge mushroom cap and stem behavior.
#[block_behavior]
pub struct HugeMushroomBlock {
    block: BlockRef,
}

const DOWN: &BoolProperty = &BlockStateProperties::DOWN;
const EAST: &BoolProperty = &BlockStateProperties::EAST;
const NORTH: &BoolProperty = &BlockStateProperties::NORTH;
const SOUTH: &BoolProperty = &BlockStateProperties::SOUTH;
const UP: &BoolProperty = &BlockStateProperties::UP;
const WEST: &BoolProperty = &BlockStateProperties::WEST;

impl HugeMushroomBlock {
    /// Creates a huge mushroom block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn placement_state(&self, world: &dyn LevelReader, pos: BlockPos) -> BlockStateId {
        let mut state = self.block.default_state();
        for direction in Direction::ALL {
            let exposed = world.get_block_state(pos.relative(direction)).get_block() != self.block;
            state = state.set_value(MultifaceBlock::face_property(direction), exposed);
        }
        state
    }
}

impl BlockBehavior for HugeMushroomBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.placement_state(context.world, context.place_pos()))
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        _world: &dyn ScheduledTickAccess,
        _pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if neighbor_state.get_block() == self.block {
            state.set_value(MultifaceBlock::face_property(direction), false)
        } else {
            state
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_blocks};

    use crate::test_support::TestLevel;

    use super::*;

    #[test]
    fn placement_hides_only_faces_joined_to_the_same_mushroom_block() {
        init_vanilla_registry();
        let pos = BlockPos::new(0, 64, 0);
        let level = TestLevel::default()
            .with_block(
                pos.below(),
                vanilla_blocks::BROWN_MUSHROOM_BLOCK.default_state(),
            )
            .with_block(
                pos.above(),
                vanilla_blocks::BROWN_MUSHROOM_BLOCK.default_state(),
            )
            .with_block(
                pos.north(),
                vanilla_blocks::BROWN_MUSHROOM_BLOCK.default_state(),
            )
            .with_block(
                pos.south(),
                vanilla_blocks::RED_MUSHROOM_BLOCK.default_state(),
            );
        let behavior = HugeMushroomBlock::new(&vanilla_blocks::BROWN_MUSHROOM_BLOCK);

        let state = behavior.placement_state(&level, pos);

        assert!(!state.get_value(DOWN));
        assert!(!state.get_value(UP));
        assert!(!state.get_value(NORTH));
        assert!(state.get_value(SOUTH));
        assert!(state.get_value(WEST));
        assert!(state.get_value(EAST));
    }

    #[test]
    fn matching_neighbor_hides_the_joined_face() {
        init_vanilla_registry();
        let behavior = HugeMushroomBlock::new(&vanilla_blocks::MUSHROOM_STEM);
        let state = vanilla_blocks::MUSHROOM_STEM.default_state();
        let level = TestLevel::default();

        let updated = behavior.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::East,
            BlockPos::ZERO.east(),
            vanilla_blocks::MUSHROOM_STEM.default_state(),
        );

        assert!(!updated.get_value(EAST));
    }

    #[test]
    fn removing_a_neighbor_does_not_restore_a_hidden_face() {
        init_vanilla_registry();
        let behavior = HugeMushroomBlock::new(&vanilla_blocks::RED_MUSHROOM_BLOCK);
        let state = vanilla_blocks::RED_MUSHROOM_BLOCK
            .default_state()
            .set_value(WEST, false);
        let level = TestLevel::default();

        let updated = behavior.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::West,
            BlockPos::ZERO.west(),
            vanilla_blocks::AIR.default_state(),
        );

        assert!(!updated.get_value(WEST));
    }
}
