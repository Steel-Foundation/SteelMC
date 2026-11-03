//! Chest and trapped chest block behavior.
// TODO: Create the chest block entity with inventory, loot table, lock and custom name support
// TODO: Resolve facing, waterlogging and double chest placement (including secondary use)
// TODO: Combine connected halves into one container/menu and check blocks and sitting cats above them
// TODO: Open the chest menu, award stats and anger nearby piglins on interaction
// TODO: Trapped chest redstone signal from the open count
// TODO: Support facing rotation and mirroring for structure transforms

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, ChestType};
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::BlockPlaceContext;
use crate::behavior::block::{BlockBehavior, schedule_water_tick_if_waterlogged};
use crate::world::ScheduledTickAccess;

/// Plain chest behavior.
#[block_behavior]
pub struct ChestBlock {
    block: BlockRef,
}

impl ChestBlock {
    /// Creates a new chest behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// The horizontal direction a double chest half joins its partner in.
    pub(super) fn connected_direction(state: BlockStateId) -> Direction {
        let facing = state.get_value(&BlockStateProperties::FACING);
        if state.get_value(&BlockStateProperties::CHEST_TYPE) == ChestType::Left {
            facing.rotate_y_clockwise()
        } else {
            facing.rotate_y_counter_clockwise()
        }
    }

    /// Joins a single chest to a facing half that points at it, and splits a
    /// half whose partner side no longer holds a connectable chest.
    pub(super) fn chest_update_shape(
        state: BlockStateId,
        direction: Direction,
        neighbor_state: BlockStateId,
        can_connect_to: impl Fn(BlockStateId) -> bool,
    ) -> BlockStateId {
        if can_connect_to(neighbor_state) && direction.is_horizontal() {
            let neighbor_type = neighbor_state.get_value(&BlockStateProperties::CHEST_TYPE);
            if state.get_value(&BlockStateProperties::CHEST_TYPE) == ChestType::Single
                && neighbor_type != ChestType::Single
                && state.get_value(&BlockStateProperties::FACING)
                    == neighbor_state.get_value(&BlockStateProperties::FACING)
                && Self::connected_direction(neighbor_state) == direction.opposite()
            {
                let opposite_type = match neighbor_type {
                    ChestType::Left => ChestType::Right,
                    ChestType::Right => ChestType::Left,
                    ChestType::Single => ChestType::Single,
                };
                return state.set_value(&BlockStateProperties::CHEST_TYPE, opposite_type);
            }
        } else if Self::connected_direction(state) == direction {
            return state.set_value(&BlockStateProperties::CHEST_TYPE, ChestType::Single);
        }
        state
    }
}

impl BlockBehavior for ChestBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
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
        schedule_water_tick_if_waterlogged(state, world, pos);
        Self::chest_update_shape(state, direction, neighbor_state, |neighbor| {
            neighbor.get_block() == self.block
        })
    }
}

/// Trapped chest behavior; connects only to other trapped chests.
#[block_behavior]
pub struct TrappedChestBlock {
    block: BlockRef,
}

impl TrappedChestBlock {
    /// Creates a new trapped chest behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for TrappedChestBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
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
        schedule_water_tick_if_waterlogged(state, world, pos);
        ChestBlock::chest_update_shape(state, direction, neighbor_state, |neighbor| {
            neighbor.get_block() == self.block
        })
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::blocks::block_state_ext::BlockStateExt as _;
    use steel_registry::blocks::properties::{BlockStateProperties, ChestType};
    use steel_registry::{init_vanilla_registry, vanilla_blocks};
    use steel_utils::{BlockPos, Direction};

    use super::ChestBlock;
    use crate::behavior::block::BlockBehavior;
    use crate::test_support::TestLevel;

    #[test]
    fn half_pointing_away_from_a_chest_becomes_single() {
        init_vanilla_registry();
        let behavior = ChestBlock::new(&vanilla_blocks::CHEST);
        // A north-facing right half joins towards the west.
        let right = vanilla_blocks::CHEST
            .default_state()
            .set_value(&BlockStateProperties::FACING, Direction::North)
            .set_value(&BlockStateProperties::CHEST_TYPE, ChestType::Right);
        let level = TestLevel::default();
        let pos = BlockPos::new(0, 64, 0);
        let result = behavior.update_shape(
            right,
            &level,
            pos,
            Direction::West,
            pos.west(),
            vanilla_blocks::STONE.default_state(),
        );
        assert_eq!(
            result.get_value(&BlockStateProperties::CHEST_TYPE),
            ChestType::Single
        );
    }
}
