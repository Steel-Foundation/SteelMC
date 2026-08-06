use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_utils::BlockStateId;

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext},
    entity::ai::path::PathComputationType,
};

/// Behavior for mud blocks.
#[block_behavior]
pub struct MudBlock {
    block: BlockRef,
}

impl MudBlock {
    /// Creates a new mud block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for MudBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
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
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;

    use crate::behavior::block::BlockBehavior;
    use crate::entity::ai::path::PathComputationType;

    use super::MudBlock;

    #[test]
    fn is_pathfindable_returns_false_for_all_types() {
        init_test_registry();
        let block = MudBlock::new(&vanilla_blocks::MUD);
        let state = vanilla_blocks::MUD.default_state();

        assert!(!block.is_pathfindable(state, PathComputationType::Land));
        assert!(!block.is_pathfindable(state, PathComputationType::Water));
        assert!(!block.is_pathfindable(state, PathComputationType::Air));
    }
}
