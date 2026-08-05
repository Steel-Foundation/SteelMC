use glam::DVec3;

use super::reduced_tick_delay;
use super::selector::{Goal, GoalControls};
use crate::entity::ai::path::PathType;
use crate::entity::{Mob, PathfinderMob, SharedEntity};

type FollowMobPredicate = Box<dyn Fn(&dyn PathfinderMob, &dyn Mob) -> bool + Send + Sync>;

pub struct FollowMobGoal {
    following_mob: Option<SharedEntity>,
    follow_predicate: FollowMobPredicate,
    speed_modifier: f64,
    time_to_recalc_path: i32,
    stop_distance: f32,
    old_water_cost: f32,
    area_size: f32,
}

impl FollowMobGoal {
    #[must_use]
    pub(crate) fn new(
        speed_modifier: f64,
        stop_distance: f32,
        area_size: f32,
        follow_predicate: impl Fn(&dyn PathfinderMob, &dyn Mob) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            following_mob: None,
            follow_predicate: Box::new(follow_predicate),
            speed_modifier,
            time_to_recalc_path: 0,
            stop_distance,
            old_water_cost: 0.0,
            area_size,
        }
    }

    fn stop_distance_sqr(&self) -> f64 {
        f64::from(self.stop_distance * self.stop_distance)
    }

    fn should_back_away(&self, mob: &dyn PathfinderMob, following_mob: &SharedEntity) -> bool {
        let mob_position = mob.position();
        let following_position = following_mob.position();
        let delta = mob_position - following_position;
        let distance_sqr = delta.length_squared();
        if distance_sqr <= f64::from(self.stop_distance) {
            return true;
        }

        following_mob.as_mob().is_some_and(|following_mob| {
            following_mob
                .mob_base()
                .controls()
                .lock()
                .look_control
                .wanted_position()
                == mob_position
        })
    }
}

impl Goal for FollowMobGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::LOOK
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(world) = mob.level() else {
            return false;
        };

        let search_box = mob.bounding_box().inflate(f64::from(self.area_size));
        let mut candidates = world.get_entities_in_aabb_matching(&search_box, |entity| {
            if entity.uuid() == mob.uuid() {
                return false;
            }
            let Some(candidate_mob) = entity.as_mob() else {
                return false;
            };
            !candidate_mob.is_invisible() && (self.follow_predicate)(mob, candidate_mob)
        });

        let Some(following_mob) = candidates.drain(..).next() else {
            return false;
        };
        self.following_mob = Some(following_mob);
        true
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(following_mob) = &self.following_mob else {
            return false;
        };

        !mob.mob_base().navigation().lock().is_done()
            && mob.position().distance_squared(following_mob.position()) > self.stop_distance_sqr()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.time_to_recalc_path = 0;
        self.old_water_cost = mob.get_pathfinding_malus(PathType::Water);
        mob.set_pathfinding_malus(PathType::Water, 0.0);
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.following_mob = None;
        mob.mob_base().navigation().lock().stop();
        mob.set_pathfinding_malus(PathType::Water, self.old_water_cost);
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let Some(following_mob) = &self.following_mob else {
            return;
        };
        if mob.is_leashed() {
            return;
        }

        let following_position = following_mob.position();
        mob.mob_base().controls().lock().look_control.set_look_at(
            DVec3::new(
                following_position.x,
                following_mob.get_eye_y(),
                following_position.z,
            ),
            10.0,
            mob.max_head_x_rot(),
        );

        self.time_to_recalc_path -= 1;
        if self.time_to_recalc_path > 0 {
            return;
        }
        self.time_to_recalc_path = reduced_tick_delay(10);

        let mob_position = mob.position();
        let delta = mob_position - following_position;
        let distance_sqr = delta.length_squared();
        if distance_sqr > self.stop_distance_sqr() {
            mob.move_to_pos(following_position, self.speed_modifier);
            return;
        }

        mob.mob_base().navigation().lock().stop();
        if self.should_back_away(mob, following_mob) {
            mob.move_to_pos(
                DVec3::new(
                    mob_position.x + delta.x,
                    mob_position.y,
                    mob_position.z + delta.z,
                ),
                self.speed_modifier,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};

    use super::*;
    use crate::entity::entities::PigEntity;

    #[test]
    fn follow_mob_goal_uses_move_and_look_controls() {
        let goal = FollowMobGoal::new(1.0, 3.0, 7.0, |_, _| true);

        assert_eq!(goal.controls(), GoalControls::MOVE | GoalControls::LOOK);
    }

    #[test]
    fn follow_mob_goal_requires_world() {
        init_test_registry();
        let mut goal = FollowMobGoal::new(1.0, 3.0, 7.0, |_, _| true);
        let mob = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert!(!goal.can_use(&mob));
    }

    #[test]
    fn follow_mob_goal_temporarily_removes_water_malus() {
        init_test_registry();
        let mut goal = FollowMobGoal::new(1.0, 3.0, 7.0, |_, _| true);
        let mob = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        mob.set_pathfinding_malus(PathType::Water, 4.0);

        goal.start(&mob);

        assert_eq!(
            mob.get_pathfinding_malus(PathType::Water).to_bits(),
            0.0_f32.to_bits()
        );

        goal.stop(&mob);

        assert_eq!(
            mob.get_pathfinding_malus(PathType::Water).to_bits(),
            4.0_f32.to_bits()
        );
    }

    #[test]
    fn follow_mob_goal_stops_when_no_navigation_is_running() {
        init_test_registry();
        let mut goal = FollowMobGoal::new(1.0, 3.0, 7.0, |_, _| true);
        let mob = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        goal.following_mob = Some(Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::new(4.0, 0.0, 0.0),
            Weak::new(),
        )));

        assert!(!goal.can_continue_to_use(&mob));
    }

    #[test]
    fn follow_mob_goal_looks_at_following_mob() {
        init_test_registry();
        let mut goal = FollowMobGoal::new(1.0, 3.0, 7.0, |_, _| true);
        let mob = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let following_mob: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::new(4.0, 0.0, 0.0),
            Weak::new(),
        ));
        goal.following_mob = Some(Arc::clone(&following_mob));

        goal.tick(&mob);

        let wanted_position = mob
            .mob_base()
            .controls()
            .lock()
            .look_control
            .wanted_position();
        assert_eq!(wanted_position.x.to_bits(), 4.0_f64.to_bits());
        assert_eq!(
            wanted_position.y.to_bits(),
            following_mob.get_eye_y().to_bits()
        );
        assert_eq!(wanted_position.z.to_bits(), 0.0_f64.to_bits());
    }
}
