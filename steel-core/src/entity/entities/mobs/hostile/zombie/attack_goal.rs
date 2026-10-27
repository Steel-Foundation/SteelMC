//! Vanilla `ZombieAttackGoal`: raises the arms while winding up an attack.

use crate::entity::ai::goal::{Goal, GoalControls, MeleeAttackGoal};
use crate::entity::{PathfinderMob, SharedEntity};

use super::ZOMBIE_RAISE_ARM_TICKS_BEFORE_ATTACK;

/// Vanilla `Zombie.ZombieAttackGoal`.
pub(crate) struct ZombieAttackGoal {
    melee: MeleeAttackGoal,
    raise_arm_ticks: i32,
}

impl ZombieAttackGoal {
    pub(crate) const fn new(speed_modifier: f64, following_target_even_if_not_seen: bool) -> Self {
        Self {
            melee: MeleeAttackGoal::new(speed_modifier, following_target_even_if_not_seen),
            raise_arm_ticks: 0,
        }
    }
}

impl Goal for ZombieAttackGoal {
    fn controls(&self) -> GoalControls {
        self.melee.controls()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.melee.can_use(mob)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.melee.can_continue_to_use(mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.melee.start(mob);
        mob.set_aggressive(true);
        self.raise_arm_ticks = 0;
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.melee.stop(mob);
        mob.set_aggressive(false);
    }

    fn requires_update_every_tick(&self) -> bool {
        self.melee.requires_update_every_tick()
    }

    fn tick(&mut self, mob: &dyn PathfinderMob, entity: &SharedEntity) {
        self.melee.tick(mob, entity);
        self.raise_arm_ticks += 1;
        let winding_up = self.raise_arm_ticks >= ZOMBIE_RAISE_ARM_TICKS_BEFORE_ATTACK
            && self.melee.get_ticks_until_next_attack() < MeleeAttackGoal::attack_interval() / 2;
        mob.set_aggressive(winding_up);
    }
}
