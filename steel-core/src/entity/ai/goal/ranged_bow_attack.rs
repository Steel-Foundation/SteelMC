//! Vanilla-shaped `RangedBowAttackGoal` — mobs holding a bow keep range, draw,
//! and shoot arrows at their target (mirrors `net.minecraft...RangedBowAttackGoal`).

use std::sync::Arc;

use glam::DVec3;
use steel_utils::types::InteractionHand;

use crate::entity::ai::goal::selector::{Goal, GoalControls};
use crate::entity::spawn_arrow_towards;
use crate::entity::{LivingEntity, PathfinderMob, SharedEntity};
use crate::world::World;

const MAX_ATTACK_DISTANCE_SQR: f64 = 15.0 * 15.0;
const RETREAT_DISTANCE_SQR: f64 = 8.0 * 8.0;
const FULL_DRAW_TICKS: i32 = 20;
/// Vanilla `AbstractSkeleton.shoot` in-flight arrow speed.
const SKELETON_ARROW_POWER: f32 = 1.6;
/// Base attack damage on skeleton arrows (vanilla `AbstractArrow` default).
const ARROW_DAMAGE: f64 = 2.0;
const SEE_TIME_BEFORE_CHARGE: i32 = 20;

pub(crate) struct RangedBowAttackGoal {
    speed_modifier: f64,
    attack_interval: i32,
    attack_time: i32,
    charge_time: i32,
    see_time: i32,
    charging: bool,
}

impl RangedBowAttackGoal {
    #[must_use]
    pub(crate) const fn new(speed_modifier: f64, attack_interval: i32) -> Self {
        Self {
            speed_modifier,
            attack_interval,
            attack_time: 0,
            charge_time: 0,
            see_time: 0,
            charging: false,
        }
    }

    fn set_charging(&mut self, mob: &dyn PathfinderMob, charging: bool) {
        self.charging = charging;
        if let Some(data) = mob.living_synced_data() {
            data.set_using_item_flag(charging);
        }
    }

    fn shoot(&mut self, mob: &dyn PathfinderMob, target: &SharedEntity, world: &Arc<World>) {
        self.set_charging(mob, false);
        mob.swing(InteractionHand::MainHand, false);

        let difficulty_id = world.difficulty() as i32;
        let uncertainty = (14 - difficulty_id * 4) as f32;
        let target_pos = target.position();
        // Vanilla aims at `target.getY(1/3)` (a third of the bounding-box
        // height), not the eyes — eye height overshoots the player's head.
        let aim = DVec3::new(
            target_pos.x,
            target_pos.y + f64::from(target.bounding_box().height()) / 3.0,
            target_pos.z,
        );
        if spawn_arrow_towards(
            world,
            mob,
            aim,
            SKELETON_ARROW_POWER,
            uncertainty,
            ARROW_DAMAGE,
        )
        .is_some()
        {
            world.play_sound_at(
                &steel_registry::sound_events::ENTITY_SKELETON_SHOOT,
                steel_protocol::packets::game::SoundSource::Hostile,
                mob.position(),
                1.0,
                1.0 / (rand::random::<f32>() * 0.4 + 0.8),
                None,
            );
        }

        self.attack_time = self.attack_interval + rand::random_range(0..20);
        self.charge_time = 0;
    }
}

impl Goal for RangedBowAttackGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::LOOK
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let holding_bow =
            mob.is_holding(&mut |stack| stack.is(&steel_registry::vanilla_items::BOW));
        let Some(target) = mob.target() else {
            return false;
        };
        holding_bow
            && target
                .as_living_entity()
                .is_some_and(LivingEntity::is_alive)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.can_use(mob)
    }

    fn start(&mut self, _mob: &dyn PathfinderMob) {
        self.attack_time = 0;
        self.charge_time = 0;
        self.see_time = 0;
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.set_charging(mob, false);
        self.charge_time = 0;
        self.see_time = 0;
        mob.mob_base().navigation().lock().stop();
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let Some(target) = mob.target() else {
            return;
        };

        let target_pos = target.position();
        mob.mob_base().controls().lock().look_control.set_look_at(
            DVec3::new(target_pos.x, target.get_eye_y(), target_pos.z),
            30.0,
            30.0,
        );

        let distance_sqr = mob.position().distance_squared(target_pos);
        let can_see = mob.has_line_of_sight_cached(target.as_ref());
        self.see_time = if can_see { self.see_time + 1 } else { 0 };

        if distance_sqr > MAX_ATTACK_DISTANCE_SQR || self.see_time < SEE_TIME_BEFORE_CHARGE {
            mob.move_to_pos(target_pos, self.speed_modifier);
        } else {
            mob.mob_base().navigation().lock().stop();
        }

        // Back away when the target closes in.
        if distance_sqr < RETREAT_DISTANCE_SQR {
            let away = mob.position() + (mob.position() - target_pos).normalize_or_zero() * 8.0;
            mob.move_to_pos(away, self.speed_modifier);
        }

        if self.attack_time > 0 {
            self.attack_time -= 1;
        }

        let in_range = distance_sqr <= MAX_ATTACK_DISTANCE_SQR;
        if !can_see || !in_range {
            if self.charging {
                self.set_charging(mob, false);
            }
            self.charge_time = 0;
            return;
        }

        if self.attack_time > 0 {
            return;
        }

        if !self.charging {
            self.set_charging(mob, true);
        }
        self.charge_time += 1;
        if self.charge_time >= FULL_DRAW_TICKS
            && let Some(world) = mob.level()
        {
            self.shoot(mob, &target, &world);
        }
    }
}
