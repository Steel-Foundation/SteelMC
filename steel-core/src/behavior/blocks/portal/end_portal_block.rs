use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::blocks::{BlockRef, block_state_ext::BlockStateExt as _, shapes::VoxelShape};
use steel_registry::dimension_type::DimensionTypeRef;
use steel_registry::vanilla_dimension_types;
use steel_utils::{BlockPos, BlockStateId, locks::SyncMutex};

use crate::behavior::BlockPlaceContext;
use crate::behavior::block::BlockBehavior;
use crate::block_entity::{
    SharedBlockEntity,
    entities::{EndGatewayBlockEntity, EndPortalBlockEntity},
};
use crate::entity::{Entity, InsideBlockEffectCollector};
use crate::portal::PortalKind;
use crate::world::LevelReader;
use crate::world::World;

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

    fn apply_entity_inside(world: &World, pos: BlockPos, entity: &dyn Entity) {
        Self::apply_entity_inside_for_dimension(world.dimension_type, pos, entity);
    }

    fn apply_entity_inside_for_dimension(
        dimension_type: DimensionTypeRef,
        pos: BlockPos,
        entity: &dyn Entity,
    ) {
        if !entity.can_use_portal(false) {
            return;
        }

        if dimension_type == &vanilla_dimension_types::THE_END
            && let Some(player) = entity.as_player()
            && !player.has_seen_credits()
        {
            player.show_end_credits();
            return;
        }

        entity.set_as_inside_portal(PortalKind::End, pos);
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

    fn has_block_entity(&self) -> bool {
        true
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Option<SharedBlockEntity> {
        Some(Arc::new(SyncMutex::new(EndPortalBlockEntity::new(
            level, pos, state,
        ))))
    }

    fn entity_inside(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        _effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        Self::apply_entity_inside(world, pos, entity);
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

    fn apply_entity_inside(world: &Arc<World>, pos: BlockPos, entity: &dyn Entity) {
        if !entity.can_use_portal(false) {
            return;
        }

        let Some(block_entity) = world.get_block_entity(pos) else {
            return;
        };
        let mut block_entity = block_entity.lock();
        let Some(gateway) = block_entity
            .as_any_mut()
            .downcast_mut::<EndGatewayBlockEntity>()
        else {
            return;
        };
        if gateway.is_cooling_down() {
            return;
        }

        entity.set_as_inside_portal(PortalKind::EndGateway, pos);
        gateway.trigger_cooldown(world);
    }
}

impl BlockBehavior for EndGatewayBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn can_be_replaced_by_fluid(&self, _state: BlockStateId, _fluid_block: BlockRef) -> bool {
        false
    }

    fn has_block_entity(&self) -> bool {
        true
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Option<SharedBlockEntity> {
        Some(Arc::new(SyncMutex::new(EndGatewayBlockEntity::new(
            level, pos, state,
        ))))
    }

    fn entity_inside(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        _effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        Self::apply_entity_inside(world, pos, entity);
    }
}

#[cfg(test)]
mod tests {
    use crate::behavior::block::BlockBehavior;
    use crate::behavior::{BlockStateBehaviorExt, init_behaviors};
    use crate::block_entity::entities::{EndGatewayBlockEntity, EndPortalBlockEntity};
    use crate::entity::{Entity, EntityBase};
    use crate::portal::PortalKind;
    use crate::test_support::TestLevel;
    use glam::DVec3;
    use std::sync::Weak;
    use steel_registry::blocks::block_state_ext::BlockStateExt as _;
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::{
        test_support::init_test_registry, vanilla_block_entity_types, vanilla_blocks,
        vanilla_dimension_types,
    };

    use super::{EndGatewayBlock, EndPortalBlock};
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

    #[test]
    fn end_portal_creates_end_portal_block_entity() {
        init_test_registry();
        let behavior = EndPortalBlock::new(&vanilla_blocks::END_PORTAL);
        let state = vanilla_blocks::END_PORTAL.default_state();
        let pos = BlockPos::new(2, 70, -4);

        assert!(behavior.has_block_entity());
        let block_entity = behavior
            .new_block_entity(Weak::new(), pos, state)
            .expect("end portal block entity");
        let guard = block_entity.lock();

        assert!(
            guard
                .as_any()
                .downcast_ref::<EndPortalBlockEntity>()
                .is_some()
        );
        assert_eq!(guard.get_type(), &vanilla_block_entity_types::END_PORTAL);
        assert_eq!(guard.get_block_pos(), pos);
        assert_eq!(guard.get_block_state(), state);
        assert!(guard.get_update_tag().is_some());
    }

    #[test]
    fn end_portal_marks_non_player_inside_end_portal() {
        init_test_registry();
        let entity = TestEntity::new();
        let pos = BlockPos::new(3, 70, 3);

        EndPortalBlock::apply_entity_inside_for_dimension(
            &vanilla_dimension_types::THE_END,
            pos,
            &entity,
        );

        let process = entity.base().portal_process().expect("portal process");
        assert_eq!(process.portal(), PortalKind::End);
        assert_eq!(process.entry_position(), pos);
    }

    #[test]
    fn end_gateway_creates_typed_block_entity() {
        init_test_registry();
        let behavior = EndGatewayBlock::new(&vanilla_blocks::END_GATEWAY);
        let state = vanilla_blocks::END_GATEWAY.default_state();
        let pos = BlockPos::new(2, 70, -4);

        assert!(behavior.has_block_entity());
        let block_entity = behavior
            .new_block_entity(Weak::new(), pos, state)
            .expect("end gateway block entity");
        let guard = block_entity.lock();

        assert!(
            guard
                .as_any()
                .downcast_ref::<EndGatewayBlockEntity>()
                .is_some()
        );
        assert_eq!(guard.get_block_pos(), pos);
        assert_eq!(guard.get_block_state(), state);
    }
}
