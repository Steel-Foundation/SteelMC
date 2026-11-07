use crate::entity::prelude::*;
use glam::DVec3;
use steel_utils::types::InteractionHand;

use super::reduced_tick_delay;
use super::selector::{Goal, GoalControls};
use crate::entity::ai::path::Path;
use crate::entity::entity_selector;
use crate::entity::{Living, Mob, SharedEntity};

const ATTACK_INTERVAL_TICKS: i32 = 20;
const COOLDOWN_BETWEEN_CAN_USE_CHECKS: i64 = 20;
const PATH_RECALC_BASE_TICKS: i32 = 4;
const PATH_RECALC_RANDOM_TICKS: i32 = 7;
const PATH_RECALC_LONG_DISTANCE_SQR: f64 = 1024.0;
const PATH_RECALC_MEDIUM_DISTANCE_SQR: f64 = 256.0;
const PATH_RECALC_LONG_DISTANCE_PENALTY: i32 = 10;
const PATH_RECALC_MEDIUM_DISTANCE_PENALTY: i32 = 5;
const PATH_RECALC_FAILED_MOVE_PENALTY: i32 = 15;
const PATHED_TARGET_RECALC_DISTANCE_SQR: f64 = 1.0;
const RANDOM_PATH_RECALC_CHANCE: f32 = 0.05;

pub(crate) struct MeleeAttackGoal {
    speed_modifier: f64,
    following_target_even_if_not_seen: bool,
    path: Option<Path>,
    pathed_target: DVec3,
    ticks_until_next_path_recalculation: i32,
    ticks_until_next_attack: i32,
    last_can_use_check: i64,
}

impl MeleeAttackGoal {
    #[must_use]
    pub(crate) const fn new(speed_modifier: f64, following_target_even_if_not_seen: bool) -> Self {
        Self {
            speed_modifier,
            following_target_even_if_not_seen,
            path: None,
            pathed_target: DVec3::ZERO,
            ticks_until_next_path_recalculation: 0,
            ticks_until_next_attack: 0,
            last_can_use_check: 0,
        }
    }

    fn check_and_perform_attack(
        &mut self,
        mob: &Mob<'_>,
        entity: &SharedEntity,
        target: &SharedEntity,
    ) {
        let Some(target_living) = target.as_living_entity() else {
            return;
        };
        if !self.can_perform_attack(mob, &target_living) {
            return;
        }

        self.reset_attack_cooldown();
        mob.swing_for_attack(InteractionHand::MainHand);
        if let Some(world) = mob.level() {
            let _ = mob.do_hurt_target(entity, &world, target);
        }
    }

    const fn reset_attack_cooldown(&mut self) {
        self.ticks_until_next_attack = Self::attack_interval();
    }

    const fn is_time_to_attack(&self) -> bool {
        self.ticks_until_next_attack <= 0
    }

    fn can_perform_attack(&self, mob: &Mob<'_>, target: &Living<'_>) -> bool {
        self.is_time_to_attack()
            && mob.is_within_melee_attack_range(target)
            && mob.has_line_of_sight_cached(target.entity())
    }

    pub(crate) const fn get_ticks_until_next_attack(&self) -> i32 {
        self.ticks_until_next_attack
    }

    const fn attack_interval() -> i32 {
        reduced_tick_delay(ATTACK_INTERVAL_TICKS)
    }

    fn has_no_pathed_target(&self) -> bool {
        self.pathed_target.x == 0.0 && self.pathed_target.y == 0.0 && self.pathed_target.z == 0.0
    }

    fn should_recalculate_path(&mut self, mob: &Mob<'_>, target: &SharedEntity) -> bool {
        let Some(target_living) = target.as_living_entity() else {
            return false;
        };
        if !self.following_target_even_if_not_seen
            && !mob.has_line_of_sight_cached(target_living.entity())
        {
            return false;
        }
        if self.ticks_until_next_path_recalculation > 0 {
            return false;
        }
        if self.has_no_pathed_target() {
            return true;
        }
        if target.position().distance_squared(self.pathed_target)
            >= PATHED_TARGET_RECALC_DISTANCE_SQR
        {
            return true;
        }

        rand::random::<f32>() < RANDOM_PATH_RECALC_CHANCE
    }

    fn recalculate_path(&mut self, mob: &Mob<'_>, target: &SharedEntity) {
        self.pathed_target = target.position();
        let random_delay = rand::random_range(0..PATH_RECALC_RANDOM_TICKS);
        self.ticks_until_next_path_recalculation = PATH_RECALC_BASE_TICKS + random_delay;

        let target_distance_sqr = mob.position().distance_squared(target.position());
        if target_distance_sqr > PATH_RECALC_LONG_DISTANCE_SQR {
            self.ticks_until_next_path_recalculation += PATH_RECALC_LONG_DISTANCE_PENALTY;
        } else if target_distance_sqr > PATH_RECALC_MEDIUM_DISTANCE_SQR {
            self.ticks_until_next_path_recalculation += PATH_RECALC_MEDIUM_DISTANCE_PENALTY;
        }

        if !mob.move_to_pos(target.position(), self.speed_modifier) {
            self.ticks_until_next_path_recalculation += PATH_RECALC_FAILED_MOVE_PENALTY;
        }

        self.ticks_until_next_path_recalculation =
            reduced_tick_delay(self.ticks_until_next_path_recalculation);
    }
}

impl Goal for MeleeAttackGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::LOOK
    }

    fn can_use(&mut self, mob: &Mob<'_>) -> bool {
        let Some(world) = mob.level() else {
            return false;
        };
        let game_time = world.game_time();
        if game_time - self.last_can_use_check < COOLDOWN_BETWEEN_CAN_USE_CHECKS {
            return false;
        }

        self.last_can_use_check = game_time;
        let Some(target) = mob.target() else {
            return false;
        };
        let Some(target_living) = target.as_living_entity() else {
            return false;
        };
        if !target_living.is_alive() {
            return false;
        }

        self.path = mob.create_path_to(target.block_position(), 0);
        self.path.is_some() || mob.is_within_melee_attack_range(&target_living)
    }

    fn can_continue_to_use(&mut self, mob: &Mob<'_>) -> bool {
        let Some(target) = mob.target() else {
            return false;
        };
        if !target.is_alive() {
            return false;
        }
        if !self.following_target_even_if_not_seen {
            return !mob.mob_base().navigation().lock().is_done();
        }
        if !mob.is_within_home_pos(target.block_position()) {
            return false;
        }

        entity_selector::no_creative_or_spectator(target.as_ref())
    }

    fn start(&mut self, mob: &Mob<'_>) {
        if let Some(path) = self.path.take() {
            mob.move_to_path(Some(path), self.speed_modifier);
        } else {
            mob.mob_base().navigation().lock().stop();
        }
        mob.set_aggressive(true);
        self.ticks_until_next_path_recalculation = 0;
        self.ticks_until_next_attack = 0;
    }

    fn stop(&mut self, mob: &Mob<'_>) {
        if mob
            .target()
            .as_ref()
            .is_some_and(|target| !entity_selector::no_creative_or_spectator(target.as_ref()))
        {
            mob.set_target(None);
        }

        mob.set_aggressive(false);
        mob.mob_base().navigation().lock().stop();
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &Mob<'_>, entity: &SharedEntity) {
        let Some(target) = mob.target() else {
            return;
        };

        let target_position = target.position();
        mob.mob_base().controls().lock().look_control.set_look_at(
            DVec3::new(target_position.x, target.eye_y(), target_position.z),
            30.0,
            30.0,
        );

        self.ticks_until_next_path_recalculation =
            (self.ticks_until_next_path_recalculation - 1).max(0);
        if self.should_recalculate_path(mob, &target) {
            self.recalculate_path(mob, &target);
        }

        self.ticks_until_next_attack = (self.ticks_until_next_attack - 1).max(0);
        self.check_and_perform_attack(mob, entity, &target);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities};

    use super::*;

    use crate::entity::ai::goal::selector::Goal;
    use crate::entity::entities::ChickenEntity;

    fn pig(id: i32, position: DVec3) -> ChickenEntity {
        ChickenEntity::new(&vanilla_entities::CHICKEN, id, position, Weak::new())
    }

    fn shared_pig(id: i32, position: DVec3) -> SharedEntity {
        Arc::new(pig(id, position))
    }

    #[test]
    fn melee_attack_goal_uses_move_and_look_controls() {
        let goal = MeleeAttackGoal::new(1.0, true);

        assert_eq!(goal.controls(), GoalControls::MOVE | GoalControls::LOOK);
        assert!(goal.requires_update_every_tick());
    }

    #[test]
    fn melee_attack_goal_requires_world_to_start() {
        init_vanilla_registry();
        let mut goal = MeleeAttackGoal::new(1.0, true);
        let mob = pig(1, DVec3::ZERO);
        let target = shared_pig(2, DVec3::new(1.0, 0.0, 0.0));
        assert!(mob.mob().set_target(Some(&target)));

        assert!(!goal.can_use(&mob.mob()));
    }

    #[test]
    fn melee_attack_goal_start_without_path_still_sets_aggressive() {
        init_vanilla_registry();
        let mut goal = MeleeAttackGoal::new(1.0, true);
        let mob = pig(1, DVec3::ZERO);

        goal.start(&mob.mob());

        assert!(mob.mob().is_aggressive());
        assert!(mob.mob().mob_base().navigation().lock().is_done());
        assert_eq!(goal.ticks_until_next_path_recalculation, 0);
        assert_eq!(goal.get_ticks_until_next_attack(), 0);
    }

    #[test]
    fn melee_attack_goal_stop_clears_aggression_and_navigation() {
        init_vanilla_registry();
        let mut goal = MeleeAttackGoal::new(1.0, true);
        let mob = pig(1, DVec3::ZERO);
        mob.mob().set_aggressive(true);
        mob.mob()
            .mob_base()
            .navigation()
            .lock()
            .set_direct_target(DVec3::new(4.0, 0.0, 0.0), 1.0);

        goal.stop(&mob.mob());

        assert!(!mob.mob().is_aggressive());
        assert!(mob.mob().mob_base().navigation().lock().is_done());
    }

    #[test]
    fn melee_attack_goal_without_unseen_following_requires_active_navigation() {
        init_vanilla_registry();
        let mut goal = MeleeAttackGoal::new(1.0, false);
        let mob = pig(1, DVec3::ZERO);
        let target = shared_pig(2, DVec3::new(4.0, 0.0, 0.0));
        assert!(mob.mob().set_target(Some(&target)));

        assert!(!goal.can_continue_to_use(&mob.mob()));

        mob.mob()
            .mob_base()
            .navigation()
            .lock()
            .set_direct_target(DVec3::new(4.0, 0.0, 0.0), 1.0);

        assert!(goal.can_continue_to_use(&mob.mob()));
    }

    #[test]
    fn melee_attack_goal_tick_recalculates_path_with_failed_move_penalty() {
        init_vanilla_registry();
        let mut goal = MeleeAttackGoal::new(1.0, true);
        let mob = Arc::new(pig(1, DVec3::ZERO));
        let mob_entity: SharedEntity = Arc::<ChickenEntity>::clone(&mob);
        let target = shared_pig(2, DVec3::new(4.0, 0.0, 0.0));
        assert!(mob.mob().set_target(Some(&target)));

        goal.tick(&mob.mob(), &mob_entity);

        assert_eq!(goal.pathed_target, target.position());
        assert!(goal.ticks_until_next_path_recalculation > 0);
        assert!(mob.mob().mob_base().navigation().lock().is_done());
    }
}
