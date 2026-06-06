use std::sync::Arc;

use glam::DVec3;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_utils::{BlockPos, BlockStateId};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext, EntityFallDamage, EntityFallOnContext,
        EntityLandingContext,
    },
    world::World,
};

const BED_BOUNCE_SCALE: f64 = 0.660_000_026_226_043_7;

/// Behavior for beds.
///
/// TODO: Add two-block placement, bed block entities, sleep interaction, and
/// invalid-dimension explosion behavior with the rest of the bed system.
#[block_behavior]
pub struct BedBlock {
    block: BlockRef,
}

impl BedBlock {
    /// Creates a bed block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    #[must_use]
    fn fall_context(context: EntityFallOnContext<'_>) -> EntityFallOnContext<'_> {
        context.with_fall_distance(context.fall_distance * 0.5)
    }

    #[must_use]
    fn velocity_after_fall(context: EntityLandingContext) -> DVec3 {
        if context.velocity.y >= 0.0 {
            return context.velocity;
        }

        let entity_factor = if context.is_living_entity { 1.0 } else { 0.8 };
        DVec3::new(
            context.velocity.x,
            -context.velocity.y * BED_BOUNCE_SCALE * entity_factor,
            context.velocity.z,
        )
    }
}

impl BlockBehavior for BedBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn fall_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        context: EntityFallOnContext<'_>,
    ) -> Option<EntityFallDamage> {
        self.default_fall_on(state, world, pos, Self::fall_context(context))
    }

    fn update_entity_movement_after_fall_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        context: EntityLandingContext,
    ) -> DVec3 {
        if context.suppresses_bounce {
            return self.default_update_entity_movement_after_fall_on(state, world, pos, context);
        }

        Self::velocity_after_fall(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use steel_registry::{sound_events, vanilla_entities};

    use crate::behavior::EntityFallOnFacts;

    fn landing(
        velocity: DVec3,
        is_living_entity: bool,
        suppresses_bounce: bool,
    ) -> EntityLandingContext {
        EntityLandingContext::new(velocity, is_living_entity, suppresses_bounce)
    }

    #[test]
    fn bed_halves_fall_distance_before_default_damage() {
        let context = BedBlock::fall_context(EntityFallOnContext::new(
            12.0,
            false,
            EntityFallOnFacts::new(
                &vanilla_entities::PLAYER,
                true,
                0.6,
                1.8,
                (
                    &sound_events::ENTITY_PLAYER_SMALL_FALL,
                    &sound_events::ENTITY_PLAYER_BIG_FALL,
                ),
            ),
            None,
        ));

        assert!((context.fall_distance - 6.0).abs() < f64::EPSILON);
        assert!(!context.suppresses_bounce);
        assert!(context.entity.is_player());
    }

    #[test]
    fn living_entities_bounce_with_bed_factor() {
        let velocity =
            BedBlock::velocity_after_fall(landing(DVec3::new(1.0, -3.0, -2.0), true, false));

        assert!((velocity.y - 1.980_000_078_678_131).abs() < f64::EPSILON);
        assert!((velocity.x - 1.0).abs() < f64::EPSILON);
        assert!((velocity.z + 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn non_living_entities_bounce_with_vanilla_reduction() {
        let velocity =
            BedBlock::velocity_after_fall(landing(DVec3::new(1.0, -3.0, -2.0), false, false));

        assert!((velocity.y - 1.584_000_062_942_505).abs() < f64::EPSILON);
    }

    #[test]
    fn upward_velocity_is_not_changed_by_bounce_logic() {
        let velocity =
            BedBlock::velocity_after_fall(landing(DVec3::new(1.0, 0.5, -2.0), true, false));

        assert_eq!(velocity, DVec3::new(1.0, 0.5, -2.0));
    }
}
