//! Vanilla `ThrowableProjectile` — the gravity/drag movement loop.

use crate::entity::RemovalReason;
use crate::entity::projectile::Projectile;

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

        // TODO: handle_first_tick_bubble_column (bubble column shove on spawn).
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
}
