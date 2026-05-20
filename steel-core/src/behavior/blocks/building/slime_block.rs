//! Slime block behavior impl

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::vanilla_damage_types;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::Entity;
use crate::entity::damage::DamageSource;
use crate::world::World;

/// Behavior for slime block
/// TODO: Bounce (`updateEntityMovementAfterFallOn`) and lateral damping (`stepOn`)
#[block_behavior]
pub struct SlimeBlock {
    block: BlockRef,
}

impl SlimeBlock {
    /// Creates a new slime block behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for SlimeBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn fall_on(
        &self,
        _state: BlockStateId,
        _world: &std::sync::Arc<World>,
        _pos: BlockPos,
        entity: &dyn Entity,
        fall_distance: f32,
    ) {
        if !entity.is_suppressing_bounce() {
            entity.cause_fall_damage(
                fall_distance,
                0.0,
                &DamageSource::environment(&vanilla_damage_types::FALL),
            );
        }
    }
}
