use std::sync::Arc;

use glam::DVec3;
use steel_macros::block_behavior;
use steel_registry::{blocks::BlockRef, vanilla_damage_types};
use steel_utils::{BlockPos, BlockStateId};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext, EntityFallDamage, EntityFallOnContext,
        EntityLandingContext,
    },
    entity::damage::DamageSource,
    world::World,
};

/// Behavior for slime blocks.
///
/// TODO: Add vanilla `stepOn` horizontal damping.
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
    fn velocity_after_fall(context: EntityLandingContext) -> DVec3 {
        if context.suppresses_bounce {
            return context.default_velocity_after_fall_on();
        }

        if context.velocity.y >= 0.0 {
            return context.velocity;
        }

        let bounce_factor = if context.is_living_entity { 1.0 } else { 0.8 };
        DVec3::new(
            context.velocity.x,
            -context.velocity.y * bounce_factor,
            context.velocity.z,
        )
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
        context: EntityFallOnContext,
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

    fn update_entity_movement_after_fall_on(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        context: EntityLandingContext,
    ) -> DVec3 {
        Self::velocity_after_fall(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn landing(
        velocity: DVec3,
        is_living_entity: bool,
        suppresses_bounce: bool,
    ) -> EntityLandingContext {
        EntityLandingContext::new(velocity, is_living_entity, suppresses_bounce)
    }

    #[test]
    fn living_entities_bounce_with_full_fall_speed() {
        let velocity =
            SlimeBlock::velocity_after_fall(landing(DVec3::new(0.25, -1.5, -0.5), true, false));

        assert_eq!(velocity, DVec3::new(0.25, 1.5, -0.5));
    }

    #[test]
    fn non_living_entities_bounce_with_vanilla_reduction() {
        let velocity =
            SlimeBlock::velocity_after_fall(landing(DVec3::new(0.0, -2.0, 0.0), false, false));

        assert_eq!(velocity, DVec3::new(0.0, 1.6, 0.0));
    }

    #[test]
    fn suppressing_bounce_uses_default_landing_velocity() {
        let velocity =
            SlimeBlock::velocity_after_fall(landing(DVec3::new(0.5, -2.0, 0.75), true, true));

        assert_eq!(velocity, DVec3::new(0.5, 0.0, 0.75));
    }

    #[test]
    fn upward_velocity_is_not_changed_by_bounce_logic() {
        let velocity =
            SlimeBlock::velocity_after_fall(landing(DVec3::new(0.0, 0.2, 0.0), true, false));

        assert_eq!(velocity, DVec3::new(0.0, 0.2, 0.0));
    }
}
