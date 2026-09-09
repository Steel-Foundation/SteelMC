use std::sync::Arc;

use steel_utils::types::Difficulty;

use crate::entity::LivingEntity;
use crate::world::World;

const MIN_VISIBILITY_DISTANCE_FOR_INVISIBLE_TARGET: f64 = 2.0;

pub(crate) type TargetingSelector = Arc<dyn Fn(&dyn LivingEntity, &World) -> bool + Send + Sync>;

#[derive(Clone)]
pub(crate) struct TargetingConditions {
    is_combat: bool,
    range: f64,
    check_line_of_sight: bool,
    test_invisible: bool,
    selector: Option<TargetingSelector>,
}

impl TargetingConditions {
    #[must_use]
    pub(crate) const fn for_combat() -> Self {
        Self::new(true)
    }

    #[must_use]
    pub(crate) const fn for_non_combat() -> Self {
        Self::new(false)
    }

    const fn new(is_combat: bool) -> Self {
        Self {
            is_combat,
            range: -1.0,
            check_line_of_sight: true,
            test_invisible: true,
            selector: None,
        }
    }

    #[must_use]
    pub(crate) const fn range(mut self, range: f64) -> Self {
        self.range = range;
        self
    }

    #[must_use]
    pub(crate) const fn ignore_line_of_sight(mut self) -> Self {
        self.check_line_of_sight = false;
        self
    }

    #[must_use]
    pub(crate) const fn ignore_invisibility_testing(mut self) -> Self {
        self.test_invisible = false;
        self
    }

    #[must_use]
    pub(crate) fn selector(
        mut self,
        selector: impl Fn(&dyn LivingEntity, &World) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.selector = Some(Arc::new(selector));
        self
    }

    #[must_use]
    pub(crate) fn test(
        &self,
        world: &World,
        targeter: Option<&dyn LivingEntity>,
        target: &dyn LivingEntity,
    ) -> bool {
        if targeter.is_some_and(|targeter| targeter.uuid() == target.uuid()) {
            return false;
        }
        if !target.can_be_seen_by_anyone() {
            return false;
        }
        if let Some(selector) = &self.selector
            && !selector(target, world)
        {
            return false;
        }

        let Some(targeter) = targeter else {
            return !self.is_combat
                || target.can_be_seen_as_enemy() && world.difficulty() != Difficulty::Peaceful;
        };

        if self.is_combat && (!targeter.can_attack(target) || targeter.is_allied_to(target)) {
            return false;
        }

        if self.range > 0.0 {
            let modifier = if self.test_invisible {
                target.get_visibility_percent(Some(targeter))
            } else {
                1.0
            };
            let visibility_distance =
                (self.range * modifier).max(MIN_VISIBILITY_DISTANCE_FOR_INVISIBLE_TARGET);
            if targeter.position().distance_squared(target.position())
                > visibility_distance * visibility_distance
            {
                return false;
            }
        }

        if self.check_line_of_sight
            && let Some(mob) = targeter.as_mob()
            && !mob.has_line_of_sight_cached(target)
        {
            return false;
        }

        true
    }
}

impl Default for TargetingConditions {
    fn default() -> Self {
        Self::for_combat()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::{init_vanilla_registry, vanilla_entities};
    use steel_utils::locks::SyncMutex;

    use super::*;
    use crate::entity::{Entity, EntityBase, LivingEntityBase, Mob, MobBase};
    use crate::test_support::fresh_test_world;

    /// A test mob that is `Mob` but not `PathfinderMob`, like flying or slime-type mobs.
    struct HoveringTestMob {
        base: EntityBase,
        living_base: LivingEntityBase,
        mob_base: MobBase,
        mob_flags: SyncMutex<i8>,
        health: SyncMutex<f32>,
    }

    impl HoveringTestMob {
        fn new(id: i32) -> Self {
            init_vanilla_registry();
            Self {
                base: EntityBase::new(
                    id,
                    DVec3::ZERO,
                    vanilla_entities::PIG.dimensions,
                    Weak::new(),
                ),
                living_base: LivingEntityBase::new(&vanilla_entities::PIG),
                mob_base: MobBase::new(),
                mob_flags: SyncMutex::new(0),
                health: SyncMutex::new(10.0),
            }
        }
    }

    crate::entity::impl_test_downcast_type!(HoveringTestMob);

    impl Entity for HoveringTestMob {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::PIG
        }
    }

    impl LivingEntity for HoveringTestMob {
        fn living_base(&self) -> &LivingEntityBase {
            &self.living_base
        }

        fn get_health(&self) -> f32 {
            *self.health.lock()
        }

        fn set_health(&self, health: f32) {
            *self.health.lock() = health;
        }
    }

    impl Mob for HoveringTestMob {
        fn mob_base(&self) -> &MobBase {
            &self.mob_base
        }

        fn mob_flags(&self) -> i8 {
            *self.mob_flags.lock()
        }

        fn set_mob_flags(&self, flags: i8) {
            *self.mob_flags.lock() = flags;
        }
    }

    #[test]
    fn sight_check_applies_to_a_mob_that_does_not_pathfind() {
        let world = fresh_test_world("targeting_sight_check_scope");
        let targeter = HoveringTestMob::new(1);
        let target = HoveringTestMob::new(2);

        assert!(
            targeter.as_pathfinder_mob().is_none(),
            "the targeter must not pathfind or this test proves nothing"
        );

        assert!(
            !TargetingConditions::for_non_combat().test(world.as_ref(), Some(&targeter), &target),
            "a mob that cannot see its target should not pick it, whether or not it pathfinds"
        );
        assert!(
            TargetingConditions::for_non_combat()
                .ignore_line_of_sight()
                .test(world.as_ref(), Some(&targeter), &target),
            "the same target should be picked once sight is not required"
        );
    }
}
