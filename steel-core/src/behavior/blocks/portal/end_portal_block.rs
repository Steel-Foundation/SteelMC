use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_utils::BlockStateId;

use crate::behavior::BlockPlaceContext;
use crate::behavior::block::BlockBehavior;

/// Vanilla `EndPortalBlock` replacement behavior.
#[block_behavior]
pub struct EndPortalBlock {
    block: BlockRef,
}

impl EndPortalBlock {
    /// Creates a new end portal block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for EndPortalBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn can_be_replaced_by_fluid(&self, _state: BlockStateId, _fluid_block: BlockRef) -> bool {
        false
    }
}

/// Vanilla `EndGatewayBlock` replacement behavior.
#[block_behavior]
pub struct EndGatewayBlock {
    block: BlockRef,
}

impl EndGatewayBlock {
    /// Creates a new end gateway block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for EndGatewayBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn can_be_replaced_by_fluid(&self, _state: BlockStateId, _fluid_block: BlockRef) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use crate::behavior::{BlockStateBehaviorExt, init_behaviors};
    use steel_registry::blocks::block_state_ext::BlockStateExt as _;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    #[test]
    fn registered_end_portal_blocks_reject_fluid_replacement() {
        init_test_registry();
        init_behaviors();

        assert!(
            vanilla_blocks::END_PORTAL
                .default_state()
                .get_static_collision_shape()
                .is_empty()
        );
        assert!(
            !vanilla_blocks::END_PORTAL
                .default_state()
                .can_be_replaced_by_fluid(&vanilla_blocks::WATER)
        );
        assert!(
            !vanilla_blocks::END_GATEWAY
                .default_state()
                .can_be_replaced_by_fluid(&vanilla_blocks::LAVA)
        );
    }
}
