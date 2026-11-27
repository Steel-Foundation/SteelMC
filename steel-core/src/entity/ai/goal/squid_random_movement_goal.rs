use std::f32::consts::TAU;

use glam::DVec3;
use steel_math::trig;
use steel_utils::Downcast;

use crate::entity::{
    Entity, LivingEntity, PathfinderMob, SharedEntity,
    ai::goal::{
        reduced_tick_delay,
        selector::{Goal, GoalControls},
    },
    entities::SquidEntity,
};

/// Ticks without action after which the squid stops moving.
const NO_ACTION_TIME_LIMIT: i32 = 100;
/// Average ticks between picking a new movement vector.
const MOVEMENT_CHANGE_INTERVAL: i32 = 50;
/// Horizontal speed of a random movement vector.
const MOVEMENT_SPEED: f32 = 0.2;
/// Downward bias of the vertical component of a random movement vector.
const MOVEMENT_VERTICAL_BASE: f32 = -0.1;

pub struct SquidRandomMovementGoal;

impl SquidRandomMovementGoal {
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Goal for SquidRandomMovementGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::EMPTY
    }

    fn can_use(&mut self, _mob: &dyn PathfinderMob) -> bool {
        true
    }

    fn tick(&mut self, mob: &dyn PathfinderMob, _entity: &SharedEntity) {
        let Some(squid) = mob.downcast_ref::<SquidEntity>() else {
            tracing::warn!("SquidRandomMovementGoal assigned to non-squid entity");
            return;
        };

        if squid.no_action_time() > NO_ACTION_TIME_LIMIT {
            squid.set_movement_vector(DVec3::ZERO);
            return;
        }

        if rand::random_range(0..reduced_tick_delay(MOVEMENT_CHANGE_INTERVAL)) != 0
            && squid.is_in_water()
            && squid.has_movement_vector()
        {
            return;
        }

        let angle = rand::random::<f32>() * TAU;
        let movement = DVec3::new(
            f64::from(trig::cos(f64::from(angle)) * MOVEMENT_SPEED),
            f64::from(MOVEMENT_VERTICAL_BASE + rand::random::<f32>() * MOVEMENT_SPEED),
            f64::from(trig::sin(f64::from(angle)) * MOVEMENT_SPEED),
        );

        squid.set_movement_vector(movement);
    }
}
