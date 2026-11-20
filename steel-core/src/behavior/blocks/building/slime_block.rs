use std::sync::Arc;

use glam::DVec3;
use steel_macros::block_behavior;
use steel_registry::{blocks::BlockRef, vanilla_damage_types};
use steel_utils::{BlockPos, BlockStateId};

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext, EntityFallDamage, EntityFallOnContext},
    entity::{Entity, damage::DamageSource},
    world::World,
};

/// Behavior for slime blocks.
#[block_behavior]
pub struct SlimeBlock {
    block: BlockRef,
}

impl SlimeBlock {
    /// Creates a slime block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    #[must_use]
    fn velocity_after_step_on(velocity: DVec3, is_stepping_carefully: bool) -> DVec3 {
        let abs_delta_y = velocity.y.abs();
        if abs_delta_y >= 0.1 || is_stepping_carefully {
            return velocity;
        }

        let scale = 0.4 + abs_delta_y * 0.2;
        DVec3::new(velocity.x * scale, velocity.y, velocity.z * scale)
    }
}

impl BlockBehavior for SlimeBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn fall_on(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        context: EntityFallOnContext<'_>,
    ) -> Option<EntityFallDamage> {
        if context.suppresses_bounce {
            None
        } else {
            Some(EntityFallDamage::new(
                context.fall_distance,
                0.0,
                DamageSource::environment(&vanilla_damage_types::FALL),
            ))
        }
    }

    fn step_on(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos, entity: &dyn Entity) {
        entity.set_velocity(Self::velocity_after_step_on(
            entity.velocity(),
            entity.is_stepping_carefully(),
        ));

        self.default_step_on(state, world, pos, entity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_on_damps_horizontal_velocity_for_non_careful_entities() {
        let velocity = SlimeBlock::velocity_after_step_on(DVec3::new(1.0, 0.05, -2.0), false);

        assert!((velocity - DVec3::new(0.41, 0.05, -0.82)).length() < 1.0e-12);
    }

    #[test]
    fn step_on_keeps_velocity_for_careful_entities() {
        let velocity = DVec3::new(1.0, 0.05, -2.0);

        assert_eq!(SlimeBlock::velocity_after_step_on(velocity, true), velocity);
    }

    #[test]
    fn step_on_keeps_horizontal_velocity_when_vertical_speed_is_large() {
        let velocity = DVec3::new(1.0, -0.1, -2.0);

        assert_eq!(
            SlimeBlock::velocity_after_step_on(velocity, false),
            velocity
        );
    }
}
