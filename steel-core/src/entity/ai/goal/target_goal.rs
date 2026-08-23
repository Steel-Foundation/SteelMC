use super::reduced_tick_delay;
use super::selector::{Goal, GoalControls};
use crate::entity::ai::targeting::TargetingConditions;
use crate::entity::{LivingEntity, Mob, PathfinderMob, SharedEntity};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_attributes;

type TargetCheck = Option<Box<dyn Fn(&dyn LivingEntity) -> bool + Send>>;

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

pub(super) fn follow_distance(mob: &dyn PathfinderMob) -> f64 {
    mob.attributes()
        .lock()
        .required_value(vanilla_attributes::FOLLOW_RANGE)
}

/// Vanilla `NearestAttackableTargetGoal`.
///
/// Scans for the nearest living entity of a specific type that passes the
/// targeting conditions, then sets it as the mob's attack target.
pub(crate) struct NearestAttackableTargetGoal {
    base: TargetGoalBase,
    target_conditions: TargetingConditions,
    target_type: EntityTypeRef,
    target_check: TargetCheck,
}

impl NearestAttackableTargetGoal {
    #[must_use]
    pub(crate) fn new(target_type: EntityTypeRef, must_see: bool) -> Self {
        Self {
            base: TargetGoalBase::new(must_see, false),
            target_conditions: TargetingConditions::for_combat(),
            target_type,
            target_check: None,
        }
    }

    /// Creates a goal with an additional per-target predicate (e.g. baby turtles).
    #[must_use]
    pub(crate) fn with_check(
        target_type: EntityTypeRef,
        must_see: bool,
        check: impl Fn(&dyn LivingEntity) -> bool + Send + 'static,
    ) -> Self {
        Self {
            base: TargetGoalBase::new(must_see, false),
            target_conditions: TargetingConditions::for_combat(),
            target_type,
            target_check: Some(Box::new(check)),
        }
    }

    pub(crate) const fn set_unseen_memory_ticks(&mut self, ticks: i32) {
        self.base.set_unseen_memory_ticks(ticks);
    }
}

impl Goal for NearestAttackableTargetGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::TARGET
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(world) = mob.level() else {
            return false;
        };

        let follow_distance = follow_distance(mob);
        let search_box = mob
            .bounding_box()
            .inflate_xyz(follow_distance, 4.0, follow_distance);

        let target_type_key = &self.target_type.key;
        let target_conditions = self.target_conditions.clone();
        let target_check = &self.target_check;

        let Some(target) =
            world.nearest_entity_in_aabb_matching(&search_box, mob.position(), |entity| {
                if entity.entity_type().key != *target_type_key {
                    return false;
                }
                let Some(living) = entity.as_living_entity() else {
                    return false;
                };
                if !target_conditions.test(world.as_ref(), Some(mob), living) {
                    return false;
                }
                if let Some(check) = target_check
                    && !check(living)
                {
                    return false;
                }
                true
            })
        else {
            return false;
        };

        self.base.set_target_mob(Some(target));
        true
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.base.can_continue_to_use(mob)
    }

    fn start(&mut self, _mob: &dyn PathfinderMob) {
        self.base.start();
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.base.stop(mob);
    }
}

/// Vanilla `HurtByTargetGoal`.
///
/// Sets the mob's target to whoever last hurt it, optionally alerting nearby
/// mobs of the same type.
pub(crate) struct HurtByTargetGoal {
    base: TargetGoalBase,
    alert_same_type: bool,
}

impl HurtByTargetGoal {
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            base: TargetGoalBase::new(true, false),
            alert_same_type: false,
        }
    }

    /// Alerts nearby mobs of the same type when this mob is hurt (vanilla
    /// `setAlertOthers`).
    pub(crate) const fn alert_same_type(mut self) -> Self {
        self.alert_same_type = true;
        self
    }
}

impl Default for HurtByTargetGoal {
    fn default() -> Self {
        Self::new()
    }
}

impl Goal for HurtByTargetGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::TARGET
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(attacker) = mob.last_hurt_by_mob() else {
            return false;
        };
        let Some(attacker_living) = attacker.as_living_entity() else {
            return false;
        };

        if !Mob::can_attack(mob, attacker_living) {
            return false;
        }

        let target_conditions = TargetingConditions::for_combat();
        let Some(world) = mob.level() else {
            return false;
        };
        if !target_conditions.test(world.as_ref(), Some(mob), attacker_living) {
            return false;
        }

        self.base.set_target_mob(Some(attacker.clone()));

        if self.alert_same_type {
            let entity_type_key = &mob.entity_type().key;
            let alert_box = mob.bounding_box().inflate_xyz(16.0, 4.0, 16.0);
            let mob_id = mob.id();
            for other in world.get_entities_in_aabb_matching(&alert_box, |entity| {
                entity.id() != mob_id && entity.entity_type().key == *entity_type_key
            }) {
                let Some(other_mob) = other.as_mob() else {
                    continue;
                };
                if other_mob.target().is_none() && Mob::can_attack(other_mob, attacker_living) {
                    other_mob.set_target(Some(&attacker));
                }
            }
        }

        true
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.base.can_continue_to_use(mob)
    }

    fn start(&mut self, _mob: &dyn PathfinderMob) {
        self.base.start();
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.base.stop(mob);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities};

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
        init_vanilla_registry();
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
        init_vanilla_registry();
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
        init_vanilla_registry();
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
        init_vanilla_registry();
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
        init_vanilla_registry();
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
        init_vanilla_registry();
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
