//! Finds players that stare at or anger an enderman.

use glam::DVec3;
use steel_utils::Downcast as _;

use super::reduced_tick_delay;
use super::selector::{Goal, GoalControls};
use super::target_goal::{TargetGoalBase, follow_distance};
use crate::entity::ai::targeting::TargetingConditions;
use crate::entity::entities::EndermanEntity;
use crate::entity::{Entity, LivingEntity, NeutralMob, PathfinderMob, SharedEntity};
use crate::world::World;

const AGGRO_DELAY: i32 = 5;
const NEAR_TELEPORT_DISTANCE_SQR: f64 = 16.0;
const FAR_TELEPORT_DISTANCE_SQR: f64 = 256.0;
const TELEPORT_DELAY: i32 = 30;
const LOOK_AT_Y_SPEED: f32 = 10.0;
const LOOK_AT_X_ANGLE: f32 = 10.0;

pub(crate) struct EndermanLookForPlayerGoal {
    target_goal: TargetGoalBase,
    pending_target: Option<SharedEntity>,
    target: Option<SharedEntity>,
    aggro_time: i32,
    teleport_time: i32,
}

impl EndermanLookForPlayerGoal {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            target_goal: TargetGoalBase::new(false, false),
            pending_target: None,
            target: None,
            aggro_time: 0,
            teleport_time: 0,
        }
    }

    fn is_anger_inducing(
        enderman: &EndermanEntity,
        target: &dyn LivingEntity,
        world: &World,
    ) -> bool {
        let stared_at = target
            .as_player()
            .is_some_and(|player| enderman.is_being_stared_by(player));
        (stared_at || enderman.is_angry_at(target, world))
            && !enderman.has_indirect_passenger(target.as_entity_event_source())
    }

    fn look_at(mob: &dyn PathfinderMob, target: &SharedEntity) {
        let position = target.position();
        mob.mob_base().controls().lock().look_control.set_look_at(
            DVec3::new(position.x, target.get_eye_y(), position.z),
            LOOK_AT_Y_SPEED,
            LOOK_AT_X_ANGLE,
        );
    }
}

impl Goal for EndermanLookForPlayerGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::TARGET
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(enderman) = mob.downcast_ref::<EndermanEntity>() else {
            return false;
        };
        let Some(world) = mob.level() else {
            return false;
        };
        let position = mob.position();
        let origin = DVec3::new(position.x, mob.get_eye_y(), position.z);
        let follow_distance = follow_distance(mob);
        self.pending_target = world
            .nearest_player(origin, follow_distance, |player| {
                Self::is_anger_inducing(enderman, player, world.as_ref())
            })
            .map(|player| -> SharedEntity { player });
        self.pending_target.is_some()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.aggro_time = reduced_tick_delay(AGGRO_DELAY);
        self.teleport_time = 0;
        if let Some(enderman) = mob.downcast_ref::<EndermanEntity>() {
            enderman.set_being_stared_at();
        }
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.pending_target = None;
        self.target_goal.stop(mob);
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(enderman) = mob.downcast_ref::<EndermanEntity>() else {
            return false;
        };
        let Some(world) = mob.level() else {
            return false;
        };

        if let Some(pending) = &self.pending_target {
            let Some(living) = pending.as_living_entity() else {
                return false;
            };
            if !Self::is_anger_inducing(enderman, living, world.as_ref()) {
                return false;
            }
            Self::look_at(mob, pending);
            return true;
        }

        if let Some(target) = &self.target {
            if enderman.has_indirect_passenger(target.as_ref()) {
                return false;
            }
            if let Some(living) = target.as_living_entity()
                && TargetingConditions::for_combat()
                    .ignore_line_of_sight()
                    .test(world.as_ref(), Some(mob), living)
            {
                return true;
            }
        }

        self.target_goal.can_continue_to_use(mob)
    }

    fn tick(&mut self, mob: &dyn PathfinderMob, _entity: &SharedEntity) {
        let Some(enderman) = mob.downcast_ref::<EndermanEntity>() else {
            return;
        };

        if mob.target().is_none() {
            self.target = None;
        }

        if self.pending_target.is_some() {
            self.aggro_time -= 1;
            if self.aggro_time <= 0 {
                self.target = self.pending_target.take();
                if let Some(target) = self.target.as_ref() {
                    let _ = mob.set_target(Some(target));
                }
                self.target_goal.start();
            }
            return;
        }

        let Some(target) = self.target.as_ref() else {
            return;
        };
        if mob.is_passenger() {
            return;
        }

        let stared_at = target
            .as_player()
            .is_some_and(|player| enderman.is_being_stared_by(player));
        let distance_sqr = mob.position().distance_squared(target.position());
        if stared_at {
            if distance_sqr < NEAR_TELEPORT_DISTANCE_SQR {
                let _ = enderman.teleport_randomly();
            }
            self.teleport_time = 0;
        } else if distance_sqr > FAR_TELEPORT_DISTANCE_SQR {
            let can_teleport = self.teleport_time >= reduced_tick_delay(TELEPORT_DELAY);
            self.teleport_time += 1;
            if can_teleport && enderman.teleport_towards(target.as_ref()) {
                self.teleport_time = 0;
            }
        }
    }
}
