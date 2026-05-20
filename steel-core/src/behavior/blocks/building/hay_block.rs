//! Hay block behavior impl

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_damage_types;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::Entity;
use crate::entity::damage::DamageSource;
use crate::world::World;

/// Behavior for hay block
#[block_behavior]
pub struct HayBlock {
    block: BlockRef,
}

impl HayBlock {
    /// Creates a new hay block behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for HayBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(
            self.block
                .default_state()
                .set_value(&BlockStateProperties::AXIS, context.clicked_face.get_axis()),
        )
    }

    fn fall_on(
        &self,
        _state: BlockStateId,
        _world: &std::sync::Arc<World>,
        _pos: BlockPos,
        entity: &dyn Entity,
        fall_distance: f32,
    ) {
        entity.cause_fall_damage(
            fall_distance,
            0.2,
            &DamageSource::environment(&vanilla_damage_types::FALL),
        );
    }
}
