//! Bespoke fox behaviour goals that read `FoxEntity` state.
//!
//! These reach the concrete `FoxEntity` from the goal's `&dyn PathfinderMob` the
//! same way the sheep and turtle goals do, via `downcast_ref`.

use std::f64::consts::TAU;
use std::sync::Arc;

use glam::DVec3;
use steel_utils::{BlockPos, Downcast as _};

use super::FoxEntity;
use crate::entity::ai::goal::{Goal, GoalControls, reduced_tick_delay};
use crate::entity::entities::objects::items::ItemEntity;
use crate::entity::{Entity, LivingEntity, Mob, PathfinderMob};
use crate::inventory::equipment::EquipmentSlot;
use crate::world::World;

/// Range, in blocks, a fox searches around itself for loose items.
const SEARCH_RANGE: f64 = 8.0;
/// Average interval, in ticks, between a fox's checks for nearby items.
const SEARCH_CHECK_TICKS: i32 = 10;
/// Speed a fox moves toward an item it wants.
const SEARCH_SPEED: f64 = 1.2;

/// Per-tick chance a resting fox perches and looks around.
const PERCH_CHANCE: f32 = 0.02;
/// Minimum and additional random look directions in one perch.
const PERCH_MIN_LOOKS: i32 = 2;
const PERCH_EXTRA_LOOKS: i32 = 3;
/// Minimum and additional random ticks a fox holds one perched look.
const PERCH_MIN_LOOK_TICKS: i32 = 80;
const PERCH_EXTRA_LOOK_TICKS: i32 = 20;

/// Randomized delay, in ticks, before a fox may fall asleep (vanilla 140).
const SLEEP_WAIT_TICKS: i32 = reduced_tick_delay(140);

/// Ticks a faceplanted fox stays down (vanilla `adjustedTickDelay(40)`).
const FACEPLANT_TICKS: i32 = reduced_tick_delay(40);

fn as_fox(mob: &dyn PathfinderMob) -> Option<&FoxEntity> {
    mob.downcast_ref::<FoxEntity>()
}

/// Returns the position of the first nearby loose item this fox would pick up
/// (vanilla `Fox.ALLOWED_ITEMS`: no pickup delay and holdable).
fn first_wanted_item(mob: &dyn PathfinderMob) -> Option<DVec3> {
    let fox = as_fox(mob)?;
    let world = mob.level()?;
    let search = mob.bounding_box().inflate(SEARCH_RANGE);
    world
        .get_entities_in_aabb(&search)
        .into_iter()
        .find_map(|entity| {
            let item = entity.downcast_ref::<ItemEntity>()?;
            (!item.has_pickup_delay() && Mob::can_hold_item(fox, &item.get_item()))
                .then(|| item.position())
        })
}

/// Vanilla `Fox.FoxSearchForItemsGoal`: an empty-mouthed fox walks to a nearby
/// item so the base looting scan can pick it up.
pub(crate) struct FoxSearchForItemsGoal;

impl Goal for FoxSearchForItemsGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(fox) = as_fox(mob) else {
            return false;
        };
        if fox.has_item_in_slot(EquipmentSlot::MainHand) {
            return false;
        }
        if Mob::target(fox).is_some() || fox.last_hurt_by_mob().is_some() || !fox.can_move() {
            return false;
        }
        if rand::random_range(0..reduced_tick_delay(SEARCH_CHECK_TICKS).max(1)) != 0 {
            return false;
        }
        first_wanted_item(mob).is_some()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        if let Some(target) = first_wanted_item(mob) {
            mob.move_to_pos(target, SEARCH_SPEED);
        }
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        if let Some(target) = first_wanted_item(mob) {
            mob.move_to_pos(target, SEARCH_SPEED);
        }
    }
}

/// Vanilla `Fox.PerchAndSearchGoal`: a fox sits and slowly looks around a few times.
pub(crate) struct PerchAndSearchGoal {
    rel_x: f64,
    rel_z: f64,
    look_time: i32,
    looks_remaining: i32,
}

impl PerchAndSearchGoal {
    pub(crate) const fn new() -> Self {
        Self {
            rel_x: 0.0,
            rel_z: 0.0,
            look_time: 0,
            looks_remaining: 0,
        }
    }

    fn reset_look(&mut self) {
        let angle = TAU * rand::random::<f64>();
        self.rel_x = angle.cos();
        self.rel_z = angle.sin();
        self.look_time = PERCH_MIN_LOOK_TICKS + rand::random_range(0..PERCH_EXTRA_LOOK_TICKS);
    }
}

impl Goal for PerchAndSearchGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::LOOK
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(fox) = as_fox(mob) else {
            return false;
        };
        fox.last_hurt_by_mob().is_none()
            && rand::random::<f32>() < PERCH_CHANCE
            && !fox.is_sleeping()
            && Mob::target(fox).is_none()
            && mob.mob_base().navigation().lock().is_done()
            && !fox.is_alertable()
            && !fox.is_pouncing()
            && !fox.is_crouching()
    }

    fn can_continue_to_use(&mut self, _mob: &dyn PathfinderMob) -> bool {
        self.looks_remaining > 0
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.reset_look();
        self.looks_remaining = PERCH_MIN_LOOKS + rand::random_range(0..PERCH_EXTRA_LOOKS);
        if let Some(fox) = as_fox(mob) {
            fox.set_sitting(true);
        }
        mob.mob_base().navigation().lock().stop();
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        if let Some(fox) = as_fox(mob) {
            fox.set_sitting(false);
        }
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        self.look_time -= 1;
        if self.look_time <= 0 {
            self.looks_remaining -= 1;
            self.reset_look();
        }

        let position = mob.position();
        let look_at = DVec3::new(
            position.x + self.rel_x,
            mob.get_eye_y(),
            position.z + self.rel_z,
        );
        mob.mob_base().controls().lock().look_control.set_look_at(
            look_at,
            mob.max_head_y_rot(),
            mob.max_head_x_rot(),
        );
    }
}

/// Vanilla `Fox.SleepGoal`: an idle fox sleeps under cover during the day.
pub(crate) struct FoxSleepGoal {
    countdown: i32,
}

impl FoxSleepGoal {
    pub(crate) fn new() -> Self {
        Self {
            countdown: rand::random_range(0..SLEEP_WAIT_TICKS.max(1)),
        }
    }

    fn can_sleep(&mut self, mob: &dyn PathfinderMob) -> bool {
        if self.countdown > 0 {
            self.countdown -= 1;
            return false;
        }
        let (Some(fox), Some(world)) = (as_fox(mob), mob.level()) else {
            return false;
        };
        world.is_bright_outside()
            && has_shelter(mob, &world)
            && !fox.is_alertable()
            && !fox.is_in_powder_snow()
    }
}

/// Returns whether the fox is under cover (vanilla `hasShelter`): the block at the
/// top of its bounding box is hidden from the sky and has a non-negative walk-target
/// value (it is on grass or bright enough).
fn has_shelter(mob: &dyn PathfinderMob, world: &Arc<World>) -> bool {
    let position = mob.position();
    let pos = BlockPos::containing(position.x, mob.bounding_box().max_y(), position.z);
    !world.can_see_sky(pos) && mob.get_walk_target_value(pos) >= 0.0
}

impl Goal for FoxSleepGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::LOOK | GoalControls::JUMP
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(fox) = as_fox(mob) else {
            return false;
        };
        let input = fox.travel_input();
        let is_still = input.sideways() == 0.0 && input.vertical() == 0.0 && input.forward() == 0.0;
        is_still && (self.can_sleep(mob) || fox.is_sleeping())
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.can_sleep(mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        if let Some(fox) = as_fox(mob) {
            fox.set_sitting(false);
            fox.set_crouching(false);
            fox.set_interested(false);
            fox.set_sleeping(true);
        }
        mob.mob_base().navigation().lock().stop();
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.countdown = rand::random_range(0..SLEEP_WAIT_TICKS.max(1));
        if let Some(fox) = as_fox(mob) {
            fox.set_sleeping(false);
            fox.set_sitting(false);
        }
    }
}

/// Vanilla `Fox.FaceplantGoal`: a fox that has faceplanted lies still for a moment
/// before getting back up.
pub(crate) struct FaceplantGoal {
    countdown: i32,
}

impl FaceplantGoal {
    pub(crate) const fn new() -> Self {
        Self { countdown: 0 }
    }
}

impl Goal for FaceplantGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::LOOK | GoalControls::JUMP
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        as_fox(mob).is_some_and(FoxEntity::is_faceplanted)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.countdown > 0 && self.can_use(mob)
    }

    fn start(&mut self, _mob: &dyn PathfinderMob) {
        self.countdown = FACEPLANT_TICKS;
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        if let Some(fox) = as_fox(mob) {
            fox.set_faceplanted(false);
        }
    }

    fn tick(&mut self, _mob: &dyn PathfinderMob) {
        self.countdown -= 1;
    }
}
