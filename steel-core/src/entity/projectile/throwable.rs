//! Vanilla `ThrowableProjectile` — the gravity/drag movement loop.
use std::sync::Arc;
use steel_registry::{blocks::block_state_ext::BlockStateExt as _, vanilla_blocks};
use steel_utils::{BlockPos, axis::Axis};

use crate::behavior::BLOCK_BEHAVIORS;
use crate::entity::projectile::Projectile;
use crate::entity::{InsideBlockEffectCollector, RemovalReason};

const DEFAULT_GRAVITY: f64 = 0.03;

/// Vanilla drag multiplier while submerged (`ThrowableProjectile.applyInertia`).
const WATER_INERTIA: f64 = 0.8;

/// Vanilla-shaped behavior shared by entities that extend `ThrowableProjectile`.
pub trait ThrowableProjectile: Projectile {
    /// Air drag multiplier applied to velocity each tick while airborne (0.99, i.e. 1% loss).
    fn get_air_drag(&self) -> f32 {
        0.99
    }

    /// Downward acceleration applied each tick while this projectile is airborne.
    fn throwable_default_gravity(&self) -> f64 {
        DEFAULT_GRAVITY
    }

    /// Vanilla `ThrowableProjectile.applyInertia` (water vs air drag).
    fn apply_inertia(&self) {
        let inertia = if self.is_in_water() {
            // VANILLA CLIENT-LOCAL: `ThrowableProjectile.tick` creates the trailing bubbles.
            WATER_INERTIA
        } else {
            f64::from(ThrowableProjectile::get_air_drag(self))
        };
        self.set_velocity(self.velocity() * inertia);
    }

    /// Reached from a subclass's `tick` as `super.tick()`. Applies gravity and
    /// drag, raycasts the move vector, moves to the hit (or full move), updates
    /// rotation, runs the `Projectile`/`Entity` base tick, then resolves the hit.
    fn throwable_projectile_tick(self: Arc<Self>) {
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

    /// Applies bubble-column effects to this projectile on its first tick.
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
