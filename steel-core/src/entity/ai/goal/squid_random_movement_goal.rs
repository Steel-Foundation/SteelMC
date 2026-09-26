use std::f32::consts::TAU;

use glam::DVec3;
use steel_math::trig;
use steel_utils::Downcast;

use crate::entity::{
    Entity, LivingEntity, PathfinderMob,
    ai::goal::{
        reduced_tick_delay,
        selector::{Goal, GoalControls},
    },
    entities::SquidEntity,
};

pub struct SquidRandomMovementGoal;

impl SquidRandomMovementGoal {
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Goal for SquidRandomMovementGoal {
    fn controls(&self) -> GoalControls {
        // Vanilla's `SquidRandomMovementGoal` never calls `setFlags`.
        GoalControls::EMPTY
    }

    fn can_use(&mut self, _mob: &dyn PathfinderMob) -> bool {
        // Always true in Squid.java
        true
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let Some(squid) = mob.downcast_ref::<SquidEntity>() else {
            tracing::warn!("SquidRandomMovementGoal assigned to non-squid entity");
            return;
        };

        if squid.no_action_time() > 100 {
            squid.set_movement_vector(DVec3::ZERO);
            return;
        }

        if rand::random_range(0..reduced_tick_delay(50)) != 0
            && squid.is_in_water()
            && squid.has_movement_vector()
        {
            return;
        }

        let angle = rand::random::<f32>() * TAU;
        let movement = DVec3::new(
            f64::from(trig::cos(f64::from(angle)) * 0.2_f32),
            f64::from(-0.1_f32 + rand::random::<f32>() * 0.2_f32),
            f64::from(trig::sin(f64::from(angle)) * 0.2_f32),
        );

        squid.set_movement_vector(movement);
    }
}
