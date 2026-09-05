//! Vanilla `ThrowableProjectile` — the gravity/drag movement loop.
use steel_registry::{blocks::block_state_ext::BlockStateExt as _, vanilla_blocks};
use steel_utils::{BlockPos, axis::Axis};

use crate::behavior::BLOCK_BEHAVIORS;
use crate::entity::projectile::Projectile;
use crate::entity::{InsideBlockEffectCollector, RemovalReason};

/// Vanilla `ThrowableProjectile.getDefaultGravity`.
const DEFAULT_GRAVITY: f64 = 0.03;

/// Vanilla drag multiplier while submerged (`ThrowableProjectile.applyInertia`).
const WATER_INERTIA: f64 = 0.8;

/// Vanilla-shaped behavior shared by entities that extend `ThrowableProjectile`.
pub trait ThrowableProjectile: Projectile {
    /// Vanilla `ThrowableProjectile.getAirDrag`.
    fn get_air_drag(&self) -> f32 {
        0.99
    }

    /// Vanilla `ThrowableProjectile.getDefaultGravity` (0.03).
    fn throwable_default_gravity(&self) -> f64 {
        DEFAULT_GRAVITY
    }

    /// Vanilla `ThrowableProjectile.applyInertia` (water vs air drag).
    fn apply_inertia(&self) {
        let inertia = if self.is_in_water() {
            // VANILLA CLIENT-LOCAL: `ThrowableProjectile.tick` creates the trailing bubbles.
            WATER_INERTIA
        } else {
            f64::from(self.get_air_drag())
        };
        self.set_velocity(self.velocity() * inertia);
    }

    /// Vanilla `ThrowableProjectile.tick`.
    ///
    /// Reached from a subclass's `tick` as `super.tick()`. Applies gravity and
    /// drag, raycasts the move vector, moves to the hit (or full move), updates
    /// rotation, runs the `Projectile`/`Entity` base tick, then resolves the hit.
    fn throwable_projectile_tick(&self) {
        // Vanilla `Entity.setOldPosAndRot()` is run by the level before ticking;
        // capture it here so `old_position()`/`old_rotation()` hold the pre-move
        // state used by `onHit` (teleport target) and `updateRotation` (lerp base).
        self.set_old_position_to_current();
        self.base().set_old_rotation_to_current();

        self.handle_first_tick_bubble_column();
        self.apply_gravity();
        self.apply_inertia();

        let hit = self.get_hit_result_on_move_vector();
        let new_position = match &hit {
            Some(result) => result.location(),
            None => self.position() + self.velocity(),
        };

        if let Err(error) = self.try_set_position(new_position) {
            log::debug!("failed to advance projectile {}: {error}", self.id());
            self.set_removed(RemovalReason::Discarded);
            return;
        }

        self.update_rotation();
        self.apply_effects_from_blocks();
        self.projectile_base_tick();

        if let Some(result) = hit
            && self.is_alive()
            && !self.is_world_change_pending()
        {
            self.hit_target_or_deflect_self(&result);
        }
    }

    /// Vanilla `ThrowableProjectile.handleFirstTickBubbleColumn`.
    fn handle_first_tick_bubble_column(&self) {
        if !self.is_first_tick() {
            return;
        }

        let Some(world) = self.level() else {
            return;
        };

        let bounds = self.bounding_box();
        let min = BlockPos::containing(
            bounds.min(Axis::X),
            bounds.min(Axis::Y),
            bounds.min(Axis::Z),
        );
        let max = BlockPos::containing(
            bounds.max(Axis::X),
            bounds.max(Axis::Y),
            bounds.max(Axis::Z),
        );

        let mut ignored_effects = InsideBlockEffectCollector::new();

        for pos in BlockPos::between_closed(min, max) {
            let state = world.get_block_state(pos);
            if state.get_block() != &vanilla_blocks::BUBBLE_COLUMN {
                continue;
            }

            BLOCK_BEHAVIORS
                .get_behavior(state.get_block())
                .entity_inside(
                    state,
                    &world,
                    pos,
                    self.as_entity_event_source(),
                    &mut ignored_effects,
                    true,
                );
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use std::sync::Arc;

    use steel_registry::blocks::{
        block_state_ext::BlockStateExt as _, properties::BlockStateProperties,
    };
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_entities};
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos};

    use crate::behavior::init_behaviors;
    use crate::entity::entities::SnowballEntity;
    use crate::entity::{Entity, SharedEntity};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn bubble_column_affects_throwable_projectile_before_its_first_movement() {
        init_vanilla_registry();
        init_behaviors();

        let world = fresh_test_world("throwable_first_tick_bubble_column");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

        let bubble_pos = BlockPos::new(8, 65, 8);
        let initial_position = DVec3::new(8.5, 65.0, 8.5);
        let bubble_column = vanilla_blocks::BUBBLE_COLUMN
            .default_state()
            .set_value(&BlockStateProperties::DRAG, false);

        assert!(world.set_block(bubble_pos, bubble_column, UpdateFlags::UPDATE_NONE));

        let snowball = Arc::new(SnowballEntity::new(
            &vanilla_entities::SNOWBALL,
            1,
            initial_position,
            Arc::downgrade(&world),
        ));
        world
            .try_add_entity(Arc::clone(&snowball) as SharedEntity)
            .expect("snowball should attach to the loaded chunk");

        assert!(snowball.is_first_tick());

        snowball.tick();

        assert!(!snowball.is_first_tick());
        assert!(
            snowball.position().y > initial_position.y,
            "the first-tick bubble-column effect should push the projectile upward before movement"
        );

        snowball
            .try_set_position(initial_position)
            .expect("snowball should return to its initial position");
        snowball.set_velocity(DVec3::ZERO);

        snowball.tick();

        assert!(
            snowball.position().y < initial_position.y,
            "after the first tick, the bubble-column effect should occur after movement"
        );
    }
}
