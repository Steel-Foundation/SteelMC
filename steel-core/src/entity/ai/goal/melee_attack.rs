use glam::DVec3;
use steel_utils::random::Random as _;
use steel_utils::types::InteractionHand;

use super::reduced_tick_delay;
use super::selector::{Goal, GoalControls};
use crate::entity::ai::path::Path;
use crate::entity::{Entity, LivingEntity, PathfinderMob, SharedEntity};

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

    fn check_and_perform_attack(&mut self, mob: &mut dyn PathfinderMob, target: &SharedEntity) {
        let can_attack = target
            .with_living(|target_living| self.can_perform_attack(mob, target_living))
            .unwrap_or(false);
        if !can_attack {
            return;
        }

        self.reset_attack_cooldown();
        mob.swing(InteractionHand::MainHand, false);
        let _ = mob.do_hurt_target(target);
    }

    const fn reset_attack_cooldown(&mut self) {
        self.ticks_until_next_attack = Self::attack_interval();
    }

    const fn is_time_to_attack(&self) -> bool {
        self.ticks_until_next_attack <= 0
    }

    fn can_perform_attack(&self, mob: &mut dyn PathfinderMob, target: &dyn LivingEntity) -> bool {
        self.is_time_to_attack()
            && mob.is_within_melee_attack_range(target)
            && mob.has_line_of_sight_cached(target)
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

    fn should_recalculate_path(
        &mut self,
        mob: &mut dyn PathfinderMob,
        target: &SharedEntity,
    ) -> bool {
        let line_of_sight_ok = target
            .with_living(|target_living| {
                self.following_target_even_if_not_seen
                    || mob.has_line_of_sight_cached(target_living)
            })
            .unwrap_or(false);
        if !line_of_sight_ok {
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

        mob.base().random().lock().next_f32() < RANDOM_PATH_RECALC_CHANCE
    }

    fn recalculate_path(&mut self, mob: &mut dyn PathfinderMob, target: &SharedEntity) {
        self.pathed_target = target.position();
        let random_delay = mob
            .base()
            .random()
            .lock()
            .next_i32_bounded(PATH_RECALC_RANDOM_TICKS);
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

    fn can_use(&mut self, mob: &mut dyn PathfinderMob) -> bool {
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
        let target_alive = target
            .with_living(|target_living| LivingEntity::is_alive(target_living))
            .unwrap_or(false);
        if !target_alive {
            return false;
        }

        self.path = mob.create_path_to(target.block_position(), 0);
        self.path.is_some()
            || target
                .with_living(|target_living| mob.is_within_melee_attack_range(target_living))
                .unwrap_or(false)
    }

    fn can_continue_to_use(&mut self, mob: &mut dyn PathfinderMob) -> bool {
        let Some(target) = mob.target() else {
            return false;
        };
        if !target.with_entity(|e| e.is_alive()) {
            return false;
        }
        if !self.following_target_even_if_not_seen {
            return !mob.mob_base().navigation.is_done();
        }
        if !mob.is_within_home_pos(target.block_position()) {
            return false;
        }

        is_no_creative_or_spectator(&target)
    }

    fn start(&mut self, mob: &mut dyn PathfinderMob) {
        if let Some(path) = self.path.take() {
            mob.move_to_path(Some(path), self.speed_modifier);
        } else {
            mob.mob_base().navigation.stop();
        }
        mob.set_aggressive(true);
        self.ticks_until_next_path_recalculation = 0;
        self.ticks_until_next_attack = 0;
    }

    fn stop(&mut self, mob: &mut dyn PathfinderMob) {
        if mob
            .target()
            .as_ref()
            .is_some_and(|target| !is_no_creative_or_spectator(target))
        {
            mob.set_target(None);
        }

        mob.set_aggressive(false);
        mob.mob_base().navigation.stop();
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &mut dyn PathfinderMob) {
        let Some(target) = mob.target() else {
            return;
        };

        let target_position = target.position();
        let target_eye_y = target.with_entity(|e| e.get_eye_y());
        mob.mob_base().controls.look_control.set_look_at(
            DVec3::new(target_position.x, target_eye_y, target_position.z),
            30.0,
            30.0,
        );

        self.ticks_until_next_path_recalculation =
            (self.ticks_until_next_path_recalculation - 1).max(0);
        if self.should_recalculate_path(mob, &target) {
            self.recalculate_path(mob, &target);
        }

        self.ticks_until_next_attack = (self.ticks_until_next_attack - 1).max(0);
        self.check_and_perform_attack(mob, &target);
    }
}

fn is_no_creative_or_spectator(entity: &SharedEntity) -> bool {
    !entity.player().is_some_and(|player| {
        let player = player.lock();
        player.is_spectator() || player.has_infinite_materials()
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::test_support::init_test_registry;

    use super::*;
    use crate::entity::ai::goal::selector::Goal;
    use crate::entity::{Entity, Mob, entities::Pig};

    fn pig(id: i32, position: DVec3) -> Pig {
        Pig::create(id, position, Weak::new())
    }

    fn shared_pig(id: i32, position: DVec3) -> SharedEntity {
        Pig::new(id, position, Weak::new())
    }

    #[test]
    fn melee_attack_goal_uses_move_and_look_controls() {
        let goal = MeleeAttackGoal::new(1.0, true);

        assert_eq!(goal.controls(), GoalControls::MOVE | GoalControls::LOOK);
        assert!(goal.requires_update_every_tick());
    }

    #[test]
    fn melee_attack_goal_requires_world_to_start() {
        init_test_registry();
        let mut goal = MeleeAttackGoal::new(1.0, true);
        let mut mob = pig(1, DVec3::ZERO);
        let target = shared_pig(2, DVec3::new(1.0, 0.0, 0.0));
        assert!(mob.set_target(Some(&target)));

        assert!(!goal.can_use(&mut mob));
    }

    #[test]
    fn melee_attack_goal_start_without_path_still_sets_aggressive() {
        init_test_registry();
        let mut goal = MeleeAttackGoal::new(1.0, true);
        let mut mob = pig(1, DVec3::ZERO);

        goal.start(&mut mob);

        assert!(mob.is_aggressive());
        assert!(mob.mob_base().navigation.is_done());
        assert_eq!(goal.ticks_until_next_path_recalculation, 0);
        assert_eq!(goal.get_ticks_until_next_attack(), 0);
    }

    #[test]
    fn melee_attack_goal_stop_clears_aggression_and_navigation() {
        init_test_registry();
        let mut goal = MeleeAttackGoal::new(1.0, true);
        let mut mob = pig(1, DVec3::ZERO);
        mob.set_aggressive(true);
        mob.mob_base()
            .navigation
            .set_direct_target(DVec3::new(4.0, 0.0, 0.0), 1.0);

        goal.stop(&mut mob);

        assert!(!mob.is_aggressive());
        assert!(mob.mob_base().navigation.is_done());
    }

    #[test]
    fn melee_attack_goal_without_unseen_following_requires_active_navigation() {
        init_test_registry();
        let mut goal = MeleeAttackGoal::new(1.0, false);
        let mut mob = pig(1, DVec3::ZERO);
        let target = shared_pig(2, DVec3::new(4.0, 0.0, 0.0));
        assert!(mob.set_target(Some(&target)));

        assert!(!goal.can_continue_to_use(&mut mob));

        mob.mob_base()
            .navigation
            .set_direct_target(DVec3::new(4.0, 0.0, 0.0), 1.0);

        assert!(goal.can_continue_to_use(&mut mob));
    }

    #[test]
    fn melee_attack_goal_tick_recalculates_path_with_failed_move_penalty() {
        init_test_registry();
        let mut goal = MeleeAttackGoal::new(1.0, true);
        let mut mob = pig(1, DVec3::ZERO);
        mob.base().random().lock().set_seed(0);
        let target = shared_pig(2, DVec3::new(4.0, 0.0, 0.0));
        assert!(mob.set_target(Some(&target)));

        goal.tick(&mut mob);

        assert_eq!(goal.pathed_target, target.position());
        assert!(goal.ticks_until_next_path_recalculation > 0);
        assert!(mob.mob_base().navigation.is_done());
    }
}
