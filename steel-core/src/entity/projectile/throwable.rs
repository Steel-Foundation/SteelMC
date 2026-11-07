//! Vanilla `ThrowableProjectile`: projectiles moved by gravity and drag.

use crate::entity::prelude::*;
use steel_registry::{blocks::block_state_ext::BlockStateExt as _, vanilla_blocks};
use steel_utils::{BlockPos, axis::Axis};

use crate::behavior::BLOCK_BEHAVIORS;
use crate::entity::projectile::{self, Parts, Projectile};
use crate::entity::{EntityProps, InsideBlockEffectCollector, RemovalReason, SharedEntity};

const DEFAULT_GRAVITY: f64 = 0.03;
const AIR_DRAG: f32 = 0.99;
const WATER_INERTIA: f64 = 0.8;

/// `ThrowableProjectile`'s overrides of the entity hooks.
#[must_use]
pub const fn entity_props<E: Parts>() -> EntityProps<E> {
    EntityProps {
        tick: |projectile, entity| tick(&projectile.projectile(), entity),
        default_gravity: |_| DEFAULT_GRAVITY,
        air_drag: |_| f64::from(AIR_DRAG),
        can_use_portal: |_, _| true,
        ..projectile::entity_props()
    }
}

/// `ThrowableProjectile.tick`: applies gravity and drag, moves to the first
/// hit on the move vector, runs the `Projectile` tick, then resolves the hit.
fn tick(projectile: &Projectile<'_>, entity: &SharedEntity) {
    // Vanilla's level stores the old position and rotation before ticking;
    // `onHit` (teleports) and `updateRotation` read them.
    projectile.set_old_position_to_current();
    projectile.base().set_old_rotation_to_current();

    handle_first_tick_bubble_column(projectile);
    projectile.apply_gravity();
    apply_inertia(projectile);

    let hit = projectile.hit_result_on_move_vector();
    let new_position = match &hit {
        Some(result) => result.location(),
        None => projectile.position() + projectile.velocity(),
    };
    if let Err(error) = projectile.try_set_position(new_position) {
        log::debug!("failed to advance projectile {}: {error}", projectile.id());
        projectile.set_removed(RemovalReason::Discarded);
        return;
    }

    projectile.update_rotation();
    projectile.apply_effects_from_blocks();
    projectile.tick();

    if let Some(result) = hit
        && projectile.is_alive()
        && !projectile.is_world_change_pending()
    {
        projectile.hit_target_or_deflect_self(entity, &result);
    }
}

/// `ThrowableProjectile.applyInertia`: water or air drag.
fn apply_inertia(projectile: &Projectile<'_>) {
    // VANILLA CLIENT-LOCAL: the trailing bubbles in water.
    let inertia = if projectile.is_in_water() {
        WATER_INERTIA
    } else {
        projectile.air_drag()
    };
    projectile.set_velocity(projectile.velocity() * inertia);
}

/// Applies bubble columns the projectile starts in on its first tick.
fn handle_first_tick_bubble_column(projectile: &Projectile<'_>) {
    if !projectile.is_first_tick() {
        return;
    }
    let Some(world) = projectile.level() else {
        return;
    };

    let bounds = projectile.bounding_box();
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
                projectile.entity(),
                &mut ignored_effects,
                true,
            );
    }
}
