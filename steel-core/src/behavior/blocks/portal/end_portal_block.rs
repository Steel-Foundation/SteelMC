use steel_macros::block_behavior;
use steel_registry::blocks::{BlockRef, block_state_ext::BlockStateExt as _, shapes::VoxelShape};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::BlockPlaceContext;
use crate::behavior::block::BlockBehavior;
use crate::entity::Entity;
use crate::world::LevelReader;

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
    fn get_entity_inside_collision_shape(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _entity: &dyn Entity,
    ) -> VoxelShape {
        state.get_static_outline_shape()
    }

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
    use crate::behavior::block::BlockBehavior;
    use crate::behavior::{BlockStateBehaviorExt, init_behaviors};
    use crate::entity::{Entity, EntityBase};
    use crate::test_support::TestLevel;
    use glam::DVec3;
    use std::sync::Weak;
    use steel_registry::blocks::block_state_ext::BlockStateExt as _;
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use super::EndPortalBlock;
    use steel_registry::vanilla_entities;
    use steel_utils::BlockPos;

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

    struct TestEntity {
        base: EntityBase,
    }

    impl TestEntity {
        fn new() -> Self {
            Self {
                base: EntityBase::new(
                    1,
                    DVec3::ZERO,
                    vanilla_entities::ITEM.dimensions,
                    Weak::new(),
                ),
            }
        }
    }

    impl Entity for TestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::ITEM
        }
    }

    #[test]
    fn end_portal_entity_inside_shape_uses_outline_shape() {
        init_test_registry();
        init_behaviors();
        let state = vanilla_blocks::END_PORTAL.default_state();
        let behavior = EndPortalBlock::new(&vanilla_blocks::END_PORTAL);
        let level = TestLevel::default();
        let entity = TestEntity::new();

        let shape =
            behavior.get_entity_inside_collision_shape(state, &level, BlockPos::ZERO, &entity);

        assert_eq!(shape, state.get_static_outline_shape());
        assert!(!shape.is_empty());
    }
}
