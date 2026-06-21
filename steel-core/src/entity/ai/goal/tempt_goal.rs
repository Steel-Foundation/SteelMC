use std::sync::Arc;

use glam::DVec3;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_attributes;

use super::reduced_tick_delay;
use super::selector::{Goal, GoalControls};
use crate::entity::ai::targeting::TargetingConditions;
use crate::entity::{Entity, LivingEntity, PathfinderMob};
use crate::player::Player;

const DEFAULT_STOP_DISTANCE: f64 = 2.5;

type TemptItemPredicate = Box<dyn Fn(&ItemStack) -> bool + Send + Sync>;

pub struct TemptGoal {
    player: Option<Arc<Player>>,
    player_position: DVec3,
    player_yaw: f32,
    player_pitch: f32,
    speed_modifier: f64,
    calm_down: i32,
    is_running: bool,
    items: TemptItemPredicate,
    can_scare: bool,
    stop_distance: f64,
}

impl TemptGoal {
    #[must_use]
    pub(crate) fn new(
        speed_modifier: f64,
        items: impl Fn(&ItemStack) -> bool + Send + Sync + 'static,
        can_scare: bool,
    ) -> Self {
        Self::with_stop_distance(speed_modifier, items, can_scare, DEFAULT_STOP_DISTANCE)
    }

    #[must_use]
    pub(crate) fn with_stop_distance(
        speed_modifier: f64,
        items: impl Fn(&ItemStack) -> bool + Send + Sync + 'static,
        can_scare: bool,
        stop_distance: f64,
    ) -> Self {
        Self {
            player: None,
            player_position: DVec3::ZERO,
            player_yaw: 0.0,
            player_pitch: 0.0,
            speed_modifier,
            calm_down: 0,
            is_running: false,
            items: Box::new(items),
            can_scare,
            stop_distance,
        }
    }

    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.is_running
    }

    fn should_follow(&self, player: &dyn LivingEntity) -> bool {
        player.is_holding(&mut |item_stack| (self.items)(item_stack))
    }

    const fn can_scare(&self) -> bool {
        self.can_scare
    }

    const fn targeting_conditions(range: f64) -> TargetingConditions {
        TargetingConditions::for_non_combat()
            .ignore_line_of_sight()
            .range(range)
    }

    fn update_player_scare_state(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(player) = &self.player else {
            return false;
        };

        if mob.position().distance_squared(player.position()) < 36.0 {
            if player.position().distance_squared(self.player_position) > 0.01 {
                return false;
            }

            let (yaw, pitch) = player.rotation();
            if (pitch - self.player_pitch).abs() > 5.0 || (yaw - self.player_yaw).abs() > 5.0 {
                return false;
            }
        } else {
            self.player_position = player.position();
        }

        let (yaw, pitch) = player.rotation();
        self.player_yaw = yaw;
        self.player_pitch = pitch;
        true
    }
}

impl Goal for TemptGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::LOOK
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        if self.calm_down > 0 {
            self.calm_down -= 1;
            return false;
        }

        let Some(world) = mob.level() else {
            return false;
        };
        let range = mob
            .attributes()
            .lock()
            .required_value(vanilla_attributes::TEMPT_RANGE);
        let targeting_conditions = Self::targeting_conditions(range);
        self.player = world.nearest_player(mob.position(), range, |player| {
            targeting_conditions.test(world.as_ref(), Some(mob), player)
                && self.should_follow(player)
        });
        self.player.is_some()
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        if self.can_scare() && !self.update_player_scare_state(mob) {
            return false;
        }

        self.can_use(mob)
    }

    fn start(&mut self, _mob: &dyn PathfinderMob) {
        if let Some(player) = &self.player {
            self.player_position = player.position();
        }
        self.is_running = true;
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.player = None;
        mob.mob_base().navigation().lock().stop();
        self.calm_down = reduced_tick_delay(100);
        self.is_running = false;
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let Some(player) = &self.player else {
            return;
        };

        let player_position = player.position();
        mob.mob_base().controls().lock().look_control.set_look_at(
            DVec3::new(player_position.x, player.get_eye_y(), player_position.z),
            mob.max_head_y_rot() + 20.0,
            mob.max_head_x_rot(),
        );

        if mob.position().distance_squared(player_position)
            < self.stop_distance * self.stop_distance
        {
            mob.mob_base().navigation().lock().stop();
        } else {
            mob.move_to_pos(player_position, self.speed_modifier);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use steel_registry::item_stack::ItemStack;
    use steel_registry::{test_support::init_test_registry, vanilla_entities, vanilla_items};

    use super::*;
    use crate::entity::entities::PigEntity;
    use crate::inventory::equipment::EquipmentSlot;

    #[test]
    fn tempt_goal_uses_move_and_look_controls() {
        let goal = TemptGoal::new(1.2, |_| false, false);

        assert_eq!(goal.controls(), GoalControls::MOVE | GoalControls::LOOK);
        assert!(!goal.is_running());
    }

    #[test]
    fn tempt_goal_should_follow_checks_both_hands() {
        init_test_registry();
        let goal = TemptGoal::new(
            1.2,
            |item_stack| item_stack.is(&vanilla_items::ITEMS.carrot),
            false,
        );
        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert!(!goal.should_follow(&pig));

        pig.with_equipment_slot_mut(EquipmentSlot::OffHand, &mut |item_stack| {
            *item_stack = ItemStack::new(&vanilla_items::ITEMS.carrot);
        });

        assert!(goal.should_follow(&pig));
    }
}
