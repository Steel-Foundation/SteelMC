//! Nether portal block behavior.

use std::sync::Arc;

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::{Entity, InsideBlockEffectCollector};
use crate::portal::PortalKind;
use crate::portal::portal_shape::{PortalShape, nether_portal_config};
use crate::world::ScheduledTickAccess;
use crate::world::World;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_blocks::AIR;
use steel_utils::axis::Axis;
use steel_utils::{BlockPos, BlockStateId, Direction};

/// Behavior for the nether portal block.
#[block_behavior]
pub struct NetherPortalBlock {
    block: BlockRef,
}
impl NetherPortalBlock {
    /// Create a new `NetherPortalBlock`
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn apply_entity_inside(pos: BlockPos, entity: &dyn Entity) {
        if entity.can_use_portal(false) {
            entity.set_as_inside_portal(PortalKind::Nether, pos);
        }
    }
}

impl BlockBehavior for NetherPortalBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let update_axis = direction.get_axis();
        let axis: Axis = state.get_value(&BlockStateProperties::HORIZONTAL_AXIS);
        let wrong_axis = axis != update_axis && update_axis != Axis::Y;

        if !wrong_axis
            && neighbor_state.get_block() != self.block
            && !PortalShape::find_any_shape(world, pos, axis, &nether_portal_config())
                .is_some_and(|s| s.is_complete())
        {
            return AIR.default_state();
        }
        state
    }

    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        None // TODO: add this functionality but has low priority
    }

    fn entity_inside(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        _effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        Self::apply_entity_inside(pos, entity);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};

    use super::*;
    use crate::entity::EntityBase;

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

    crate::entity::impl_test_downcast_type!(TestEntity);

    impl Entity for TestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::ITEM
        }
    }

    #[test]
    fn nether_portal_marks_entity_inside_portal() {
        init_test_registry();
        let entity = TestEntity::new();
        let pos = BlockPos::new(3, 70, 3);

        NetherPortalBlock::apply_entity_inside(pos, &entity);

        let process = entity.base().portal_process().expect("portal process");
        assert_eq!(process.portal(), PortalKind::Nether);
        assert_eq!(process.entry_position(), pos);
    }
}
