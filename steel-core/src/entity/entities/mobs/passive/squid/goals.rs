//! Vanilla `Squid.SquidRandomMovementGoal` and `Squid.SquidFleeGoal`.

use std::f32::consts::TAU;

use glam::DVec3;
use steel_math::trig;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::fluid::FluidStateExt as _;
use steel_utils::{BlockPos, Downcast as _, TICKS_PER_SECOND};

use super::SquidEntity;
use crate::entity::ai::goal::{Goal, GoalControls, reduced_tick_delay};
use crate::entity::prelude::*;
use crate::entity::{Mob, SharedEntity};
use crate::world::LevelReader as _;

const FLEE_SPEED: f64 = 3.0;
const FLEE_MIN_DISTANCE: f64 = 5.0;

/// Swims in a random direction now and then, or keeps still when idle.
pub(super) struct SquidRandomMovementGoal;

impl Goal for SquidRandomMovementGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::EMPTY
    }

    fn can_use(&mut self, _mob: &Mob<'_>) -> bool {
        true
    }

    fn tick(&mut self, mob: &Mob<'_>, _entity: &SharedEntity) {
        let Some(squid) = mob.entity().downcast_ref::<SquidEntity>() else {
            return;
        };
        if mob.no_action_time() > 100 {
            squid.set_movement_vector(DVec3::ZERO);
        } else if rand::random_range(0..reduced_tick_delay(50)) == 0
            || !squid.is_in_water()
            || !squid.has_movement_vector()
        {
            let angle = rand::random::<f32>() * TAU;
            squid.set_movement_vector(DVec3::new(
                f64::from(trig::cos(angle) * 0.2),
                f64::from(-0.1 + rand::random::<f32>() * 0.2),
                f64::from(trig::sin(angle) * 0.2),
            ));
        }
    }
}

/// Swims away from the mob that last hurt the squid.
pub(super) struct SquidFleeGoal {
    flee_ticks: i32,
}

impl SquidFleeGoal {
    pub(super) const fn new() -> Self {
        Self { flee_ticks: 0 }
    }
}

impl Goal for SquidFleeGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::EMPTY
    }

    fn can_use(&mut self, mob: &Mob<'_>) -> bool {
        mob.is_in_water()
            && mob
                .last_hurt_by_mob()
                .is_some_and(|attacker| mob.distance_to_sqr(attacker.position()) < 100.0)
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn start(&mut self, _mob: &Mob<'_>) {
        self.flee_ticks = 0;
    }

    fn tick(&mut self, mob: &Mob<'_>, _entity: &SharedEntity) {
        self.flee_ticks += 1;
        let Some(attacker) = mob.last_hurt_by_mob() else {
            return;
        };
        let Some(squid) = mob.entity().downcast_ref::<SquidEntity>() else {
            return;
        };
        let Some(world) = mob.level() else {
            return;
        };

        let squid_pos = squid.position();
        let mut flee_to = squid_pos - attacker.position();
        let block_state = world.get_block_state(BlockPos::from(squid_pos + flee_to));
        if !block_state.get_fluid_state().is_water() && !block_state.is_air() {
            return;
        }
        let distance = flee_to.length();
        if distance > 0.0 {
            // Vanilla calls `fleeTo.normalize()` and drops the result, so the
            // vector stays unnormalized.
            let mut avoid_speed = FLEE_SPEED;
            if distance > FLEE_MIN_DISTANCE {
                avoid_speed -= (distance - FLEE_MIN_DISTANCE) / FLEE_MIN_DISTANCE;
            }
            if avoid_speed > 0.0 {
                flee_to *= avoid_speed;
            }
        }
        if block_state.is_air() {
            flee_to.y = 0.0;
        }
        squid.set_movement_vector(flee_to / f64::from(TICKS_PER_SECOND));
        // VANILLA CLIENT-LOCAL: the bubble particle every ten ticks.
    }
}
