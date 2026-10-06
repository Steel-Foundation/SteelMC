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
            && let Some(pathfinder) = targeter.as_pathfinder_mob()
            && !pathfinder.has_line_of_sight_cached(target)
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
