//! Vanilla `ThrowableProjectile`: projectiles moved by gravity and drag.

use crate::entity::component::definition::TickCtx;
use crate::entity::prelude::*;
use steel_registry::{blocks::block_state_ext::BlockStateExt as _, vanilla_blocks};
use steel_utils::{BlockPos, axis::Axis};

use crate::behavior::BLOCK_BEHAVIORS;
use crate::entity::component::definition::Definition;
use crate::entity::component::definition::Tick;
use crate::entity::component::pipeline::{Flow, Inner, StageId};
use crate::entity::projectile::{self, Parts, Projectile};
use crate::entity::{EntityProps, InsideBlockEffectCollector, RemovalReason};

const WATER_INERTIA: f32 = 0.8;

/// `ThrowableProjectile`'s overrides of the entity hooks.
#[must_use]
pub const fn entity_props<E: Parts>() -> EntityProps<E> {
    EntityProps {
        default_gravity: |_| 0.03,
        air_drag: |_| 0.99,
        can_use_portal: |_, _| true,
        ..projectile::entity_props()
    }
}

/// The `ThrowableProjectile.tick` stage, around `Projectile`'s.
pub const TICK: StageId = StageId("throwable.tick");

/// Vanilla `ThrowableProjectile`'s chains, on `Projectile`'s.
#[must_use]
pub fn definition<E: Parts>() -> Definition<E> {
    fn run<E: Parts>(entity: &E, ctx: &mut TickCtx<'_>, inner: Inner<'_, E, Tick>) -> Flow {
        tick(&entity.projectile(), || inner(entity, ctx))
    }
    let definition = projectile::definition();
    Definition {
        tick: definition.tick.around(TICK, run::<E>),
        ..definition
    }
}

/// `ThrowableProjectile.tick`: applies gravity and drag, moves to the first
/// hit on the move vector, runs `super.tick()`, then resolves the hit.
fn tick(projectile: &Projectile<'_>, super_tick: impl FnOnce() -> Flow) -> Flow {
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
        // Vanilla's `return` only leaves this layer; subclass code after
        // `super.tick()` still runs.
        return Flow::Continue;
    }

    projectile.update_rotation();
    projectile.apply_effects_from_blocks();
    let flow = super_tick();

    if let Some(result) = hit
        && projectile.is_alive()
        && !projectile.is_world_change_pending()
    {
        projectile.hit_target_or_deflect_self(&result);
    }
    flow
}

/// `ThrowableProjectile.applyInertia`: water or air drag.
fn apply_inertia(projectile: &Projectile<'_>) {
    // VANILLA CLIENT-LOCAL: the trailing bubbles in water.
    let inertia = if projectile.is_in_water() {
        WATER_INERTIA
    } else {
        projectile.air_drag()
    };
    projectile.set_velocity(projectile.velocity() * f64::from(inertia));
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

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use std::sync::Arc;

    use steel_registry::blocks::{
        block_state_ext::BlockStateExt as _, properties::BlockStateProperties,
    };
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_entities};
    use steel_utils::BlockPos;
    use steel_utils::types::UpdateFlags;

    use super::handle_first_tick_bubble_column;
    use crate::behavior::init_behaviors;
    use crate::entity::SharedEntity;
    use crate::entity::entities::SnowballEntity;
    use crate::entity::prelude::*;
    use crate::test_support::fresh_loaded_test_world;

    #[test]
    fn bubble_column_affects_throwable_projectile_before_its_first_movement() {
        init_vanilla_registry();
        init_behaviors();

        let world_fixture = fresh_loaded_test_world("throwable_first_tick_bubble_column");
        let world = &world_fixture.world;

        let bubble_pos = BlockPos::new(8, 65, 8);
        let initial_position = DVec3::new(8.5, 65.0, 8.5);
        let bubble_column = vanilla_blocks::BUBBLE_COLUMN
            .default_state()
            .set_value(&BlockStateProperties::DRAG, false);

        assert!(world.set_block(bubble_pos, bubble_column, UpdateFlags::UPDATE_NONE));

        let snowball = SnowballEntity::new(
            &vanilla_entities::SNOWBALL,
            1,
            initial_position,
            Arc::downgrade(world),
        );
        world
            .try_add_entity(Arc::clone(&snowball) as SharedEntity)
            .expect("snowball should attach to the loaded chunk");

        assert!(snowball.is_first_tick());

        Arc::clone(&snowball).tick();

        assert!(!snowball.is_first_tick());
        assert!(
            snowball.position().y > initial_position.y,
            "the first-tick bubble-column effect should push the projectile upward before movement"
        );

        snowball
            .try_set_position(initial_position)
            .expect("snowball should return to its initial position");
        snowball.set_velocity(DVec3::ZERO);

        Arc::clone(&snowball).tick();

        assert!(
            snowball.position().y < initial_position.y,
            "after the first tick, the bubble-column effect should occur after movement"
        );
    }

    #[test]
    fn first_tick_bubble_column_does_not_clamp_projectile_velocity() {
        init_vanilla_registry();
        init_behaviors();

        let world_fixture = fresh_loaded_test_world("throwable_first_tick_bubble_column_velocity");
        let world = &world_fixture.world;

        let bubble_pos = BlockPos::new(8, 65, 8);
        let initial_position = DVec3::new(8.5, 65.0, 8.5);

        for (open_above, drag_down, initial_y, expected_y) in [
            (true, false, 2.0, 2.1),
            (true, true, -2.0, -2.03),
            (false, false, 2.0, 2.06),
            (false, true, -2.0, -2.03),
        ] {
            let bubble_column = vanilla_blocks::BUBBLE_COLUMN
                .default_state()
                .set_value(&BlockStateProperties::DRAG, drag_down);

            assert!(world.set_block(bubble_pos, bubble_column, UpdateFlags::UPDATE_NONE,));

            let above_state = if open_above {
                vanilla_blocks::AIR.default_state()
            } else {
                vanilla_blocks::WATER.default_state()
            };

            // The block may already have this state from the previous case.
            world.set_block(bubble_pos.above(), above_state, UpdateFlags::UPDATE_NONE);

            let snowball = SnowballEntity::new(
                &vanilla_entities::SNOWBALL,
                1,
                initial_position,
                Arc::downgrade(world),
            );
            snowball.set_velocity(DVec3::new(0.25, initial_y, -0.25));

            assert!(snowball.is_first_tick());
            handle_first_tick_bubble_column(&snowball.projectile());

            let expected = DVec3::new(0.25, expected_y, -0.25);
            let actual = snowball.velocity();

            assert!(
                (actual - expected).abs().max_element() < 1.0e-12,
                "open_above={open_above}, drag_down={drag_down}: \
                 expected {expected:?}, got {actual:?}"
            );
        }
    }
}
