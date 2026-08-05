use super::reduced_tick_delay;
use crate::entity::ai::targeting::TargetingConditions;
use crate::entity::{LivingEntity, Mob, PathfinderMob, SharedEntity};
use steel_registry::vanilla_attributes;

const DEFAULT_UNSEEN_MEMORY_TICKS: i32 = 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReachCache {
    Empty,
    CanReach,
    CantReach,
}

pub(super) struct TargetGoalBase {
    must_see: bool,
    must_reach: bool,
    reach_cache: ReachCache,
    reach_cache_time: i32,
    unseen_ticks: i32,
    target_mob: Option<SharedEntity>,
    unseen_memory_ticks: i32,
}

impl TargetGoalBase {
    #[must_use]
    pub(super) const fn new(must_see: bool, must_reach: bool) -> Self {
        Self {
            must_see,
            must_reach,
            reach_cache: ReachCache::Empty,
            reach_cache_time: 0,
            unseen_ticks: 0,
            target_mob: None,
            unseen_memory_ticks: DEFAULT_UNSEEN_MEMORY_TICKS,
        }
    }

    pub(super) const fn set_unseen_memory_ticks(&mut self, unseen_memory_ticks: i32) {
        self.unseen_memory_ticks = unseen_memory_ticks;
    }

    pub(super) fn set_target_mob(&mut self, target_mob: Option<SharedEntity>) {
        self.target_mob = target_mob;
    }

    pub(super) fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(target) = mob.target().or_else(|| self.target_mob.clone()) else {
            return false;
        };
        let Some(target_living) = target.as_living_entity() else {
            return false;
        };

        if !Mob::can_attack(mob, target_living) || mob.is_allied_to(target_living) {
            return false;
        }

        let follow_distance = follow_distance(mob);
        if mob.position().distance_squared(target.position()) > follow_distance * follow_distance {
            return false;
        }

        if self.must_see && !self.update_unseen_ticks(mob, target_living) {
            return false;
        }

        mob.set_target(Some(&target))
    }

    pub(super) const fn start(&mut self) {
        self.reach_cache = ReachCache::Empty;
        self.reach_cache_time = 0;
        self.unseen_ticks = 0;
    }

    pub(super) fn stop(&mut self, mob: &dyn PathfinderMob) {
        mob.set_target(None);
        self.target_mob = None;
    }

    pub(super) fn can_attack(
        &mut self,
        mob: &dyn PathfinderMob,
        target: Option<&dyn LivingEntity>,
        target_conditions: &TargetingConditions,
    ) -> bool {
        let Some(target) = target else {
            return false;
        };
        let Some(world) = mob.level() else {
            return false;
        };

        if !target_conditions.test(world.as_ref(), Some(mob), target) {
            return false;
        }
        if !mob.is_within_home_pos(target.block_position()) {
            return false;
        }

        if self.must_reach && !self.can_reach(mob, target) {
            return false;
        }

        true
    }

    fn update_unseen_ticks(&mut self, mob: &dyn PathfinderMob, target: &dyn LivingEntity) -> bool {
        if mob.has_line_of_sight_cached(target) {
            self.unseen_ticks = 0;
            return true;
        }

        self.unseen_ticks += 1;
        self.unseen_ticks <= reduced_tick_delay(self.unseen_memory_ticks)
    }

    fn can_reach(&mut self, mob: &dyn PathfinderMob, target: &dyn LivingEntity) -> bool {
        self.reach_cache_time -= 1;
        if self.reach_cache_time <= 0 {
            self.reach_cache = ReachCache::Empty;
        }

        if self.reach_cache == ReachCache::Empty {
            self.reach_cache = if self.check_reach(mob, target) {
                ReachCache::CanReach
            } else {
                ReachCache::CantReach
            };
        }

        self.reach_cache == ReachCache::CanReach
    }

    fn check_reach(&mut self, mob: &dyn PathfinderMob, target: &dyn LivingEntity) -> bool {
        self.reach_cache_time = reduced_tick_delay(10 + rand::random_range(0..5));
        mob.can_reach_living_target(target)
    }
}

fn follow_distance(mob: &dyn PathfinderMob) -> f64 {
    mob.attributes()
        .lock()
        .required_value(vanilla_attributes::FOLLOW_RANGE)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};

    use super::*;
    use crate::entity::ai::targeting::TargetingConditions;
    use crate::entity::{Mob, entities::PigEntity};

    fn pig(id: i32, position: DVec3) -> Arc<PigEntity> {
        Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            id,
            position,
            Weak::new(),
        ))
    }

    fn target_living(target: &SharedEntity) -> &dyn LivingEntity {
        let Some(living) = target.as_living_entity() else {
            panic!("test target should be a living entity");
        };
        living
    }

    #[test]
    fn target_goal_base_continues_with_existing_mob_target() {
        init_test_registry();
        let mob = pig(1, DVec3::ZERO);
        let target: SharedEntity = pig(2, DVec3::new(2.0, 0.0, 0.0));
        assert!(mob.set_target(Some(&target)));
        let mut goal = TargetGoalBase::new(false, false);

        goal.start();

        assert!(goal.can_continue_to_use(mob.as_ref()));
        let Some(stored_target) = mob.target() else {
            panic!("target should remain set");
        };
        assert_eq!(stored_target.uuid(), target.uuid());
    }

    #[test]
    fn target_goal_base_restores_stored_target_while_continuing() {
        init_test_registry();
        let mob = pig(1, DVec3::ZERO);
        let target: SharedEntity = pig(2, DVec3::new(2.0, 0.0, 0.0));
        let mut goal = TargetGoalBase::new(false, false);
        goal.set_target_mob(Some(target.clone()));

        assert!(mob.target().is_none());
        assert!(goal.can_continue_to_use(mob.as_ref()));

        let Some(stored_target) = mob.target() else {
            panic!("stored target should be copied onto the mob");
        };
        assert_eq!(stored_target.uuid(), target.uuid());
    }

    #[test]
    fn target_goal_base_forgets_unseen_target_after_memory_ticks() {
        init_test_registry();
        let mob = pig(1, DVec3::ZERO);
        let target: SharedEntity = pig(2, DVec3::new(2.0, 0.0, 0.0));
        assert!(mob.set_target(Some(&target)));
        let mut goal = TargetGoalBase::new(true, false);
        goal.set_unseen_memory_ticks(2);
        goal.start();

        assert!(goal.can_continue_to_use(mob.as_ref()));
        assert!(!goal.can_continue_to_use(mob.as_ref()));
    }

    #[test]
    fn target_goal_base_stop_clears_mob_and_stored_target() {
        init_test_registry();
        let mob = pig(1, DVec3::ZERO);
        let target: SharedEntity = pig(2, DVec3::new(2.0, 0.0, 0.0));
        assert!(mob.set_target(Some(&target)));
        let mut goal = TargetGoalBase::new(false, false);
        goal.set_target_mob(Some(target));

        goal.stop(mob.as_ref());

        assert!(mob.target().is_none());
        assert!(goal.target_mob.is_none());
    }

    #[test]
    fn target_goal_base_can_attack_requires_world() {
        init_test_registry();
        let mob = pig(1, DVec3::ZERO);
        let target: SharedEntity = pig(2, DVec3::new(2.0, 0.0, 0.0));
        let mut goal = TargetGoalBase::new(false, false);
        let target_conditions = TargetingConditions::for_combat().ignore_line_of_sight();

        assert!(!goal.can_attack(
            mob.as_ref(),
            Some(target_living(&target)),
            &target_conditions
        ));
    }

    #[test]
    fn target_goal_base_caches_unreachable_targets() {
        init_test_registry();
        let mob = pig(1, DVec3::ZERO);
        let target: SharedEntity = pig(2, DVec3::new(2.0, 0.0, 0.0));
        let mut goal = TargetGoalBase::new(false, true);

        assert!(!goal.can_reach(mob.as_ref(), target_living(&target)));
        assert_eq!(goal.reach_cache, ReachCache::CantReach);
        let first_reach_cache_time = goal.reach_cache_time;

        assert!(!goal.can_reach(mob.as_ref(), target_living(&target)));
        assert_eq!(goal.reach_cache, ReachCache::CantReach);
        assert_eq!(goal.reach_cache_time, first_reach_cache_time - 1);
    }
}
