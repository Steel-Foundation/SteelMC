use super::selector::{Goal, GoalControls};
use crate::entity::Mob;
use crate::inventory::equipment::EquipmentSlot;

pub struct RestrictSunGoal;

impl RestrictSunGoal {
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Goal for RestrictSunGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::EMPTY
    }

    fn can_use(&mut self, mob: &Mob<'_>) -> bool {
        let Some(world) = mob.level() else {
            return false;
        };

        world.is_bright_outside() && !mob.has_item_in_slot(EquipmentSlot::Head)
    }

    fn start(&mut self, mob: &Mob<'_>) {
        mob.mob_base().navigation().lock().set_avoid_sun(true);
    }

    fn stop(&mut self, mob: &Mob<'_>) {
        mob.mob_base().navigation().lock().set_avoid_sun(false);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities};

    use super::*;
    use crate::entity::prelude::*;

    use crate::entity::entities::ChickenEntity;

    fn pig() -> Arc<ChickenEntity> {
        ChickenEntity::new(&vanilla_entities::CHICKEN, 1, DVec3::ZERO, Weak::new())
    }

    #[test]
    fn restrict_sun_goal_claims_no_controls_like_vanilla() {
        let goal = RestrictSunGoal::new();

        assert_eq!(goal.controls(), GoalControls::EMPTY);
    }

    #[test]
    fn restrict_sun_goal_requires_world() {
        init_vanilla_registry();
        let mut goal = RestrictSunGoal::new();

        assert!(!goal.can_use(&pig().mob()));
    }

    #[test]
    fn restrict_sun_goal_toggles_navigation_avoid_sun() {
        init_vanilla_registry();
        let mut goal = RestrictSunGoal::new();
        let pig = pig();

        assert!(!pig.mob().mob_base().navigation().lock().avoid_sun());

        goal.start(&pig.mob());
        assert!(pig.mob().mob_base().navigation().lock().avoid_sun());

        goal.stop(&pig.mob());
        assert!(!pig.mob().mob_base().navigation().lock().avoid_sun());
    }
}
