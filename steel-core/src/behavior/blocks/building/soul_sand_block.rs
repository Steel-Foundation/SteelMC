use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_utils::BlockStateId;

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext},
    entity::ai::path::PathComputationType,
};

/// Soul sand. Mobs will not pathfind through this block.
#[block_behavior]
pub struct SoulSandBlock {
    block: BlockRef,
}

impl SoulSandBlock {
    /// Creates a new soul sand block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for SoulSandBlock {
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
    use steel_registry::init_vanilla_registry;
    use steel_registry::vanilla_blocks;

    use crate::behavior::block::BlockBehavior;
    use crate::entity::ai::path::PathComputationType;

    use super::SoulSandBlock;

    #[test]
    fn is_pathfindable_returns_false_for_all_types() {
        init_vanilla_registry();
        let block = SoulSandBlock::new(&vanilla_blocks::SOUL_SAND);
        let state = vanilla_blocks::SOUL_SAND.default_state();

        assert!(!block.is_pathfindable(state, PathComputationType::Land));
        assert!(!block.is_pathfindable(state, PathComputationType::Water));
        assert!(!block.is_pathfindable(state, PathComputationType::Air));
    }
}
