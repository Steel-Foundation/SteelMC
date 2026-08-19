use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_utils::{BlockPos, BlockStateId, Direction};

use super::snowy_block::{snowy_placement_state, update_snowy_shape};
use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::ScheduledTickAccess;

/// Behavior for mycelium blocks.
#[block_behavior]
pub struct MyceliumBlock {
    block: BlockRef,
}

impl MyceliumBlock {
    /// Creates a new mycelium block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for MyceliumBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(snowy_placement_state(self.block, context))
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
        update_snowy_shape(state, direction, neighbor_state)
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::blocks::block_state_ext::BlockStateExt;
    use steel_registry::blocks::properties::BlockStateProperties;
    use steel_registry::{init_vanilla_registry, vanilla_blocks};
    use steel_utils::{BlockPos, Direction};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::TestLevel;

    #[test]
    fn mycelium_block_updates_snowy_state() {
        init_vanilla_registry();
        init_behaviors();

        let level = TestLevel::default();
        let pos = BlockPos::new(0, 64, 0);
        let behavior = MyceliumBlock::new(&vanilla_blocks::MYCELIUM);

        let non_snowy = vanilla_blocks::MYCELIUM.default_state();
        let snowy = behavior.update_shape(
            non_snowy,
            &level,
            pos,
            Direction::Up,
            pos.above(),
            vanilla_blocks::SNOW.default_state(),
        );
        assert!(snowy.get_value(&BlockStateProperties::SNOWY));

        let cleared = behavior.update_shape(
            snowy,
            &level,
            pos,
            Direction::Up,
            pos.above(),
            vanilla_blocks::AIR.default_state(),
        );
        assert!(!cleared.get_value(&BlockStateProperties::SNOWY));
    }
}
