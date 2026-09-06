//! Bespoke fox behaviour goals that read `FoxEntity` state.
//!
//! These reach the concrete `FoxEntity` from the goal's `&dyn PathfinderMob` the
//! same way the sheep and turtle goals do, via `downcast_ref`.

use std::f64::consts::{PI, TAU};
use std::sync::Arc;

use glam::DVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::{sound_events, vanilla_attributes, vanilla_blocks, vanilla_entities};
use steel_utils::{BlockPos, Downcast as _, wrap_degrees};

use super::FoxEntity;
use crate::entity::ai::goal::{Goal, GoalControls, MeleeAttackGoal, reduced_tick_delay};
use crate::entity::ai::targeting::TargetingConditions;
use crate::entity::entities::objects::items::ItemEntity;
use crate::entity::{Entity, LivingEntity, Mob, PathfinderMob, SharedEntity};
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

/// Pounce arc: horizontal reach toward the target and the upward kick added to
/// the fox's velocity when it launches (vanilla `deltaMovement.add(uv*0.8, 0.9, ..)`).
const POUNCE_LEAP_HORIZONTAL: f64 = 0.8;
const POUNCE_LEAP_VERTICAL: f64 = 0.9;
/// Distance, in blocks, at which a pouncing fox strikes its target.
const POUNCE_HIT_DISTANCE: f64 = 2.0;
/// Head-turn speeds (yaw, then pitch) the pounce turns the fox's head at.
const POUNCE_LOOK_Y_SPEED: f32 = 60.0;
const POUNCE_LOOK_X_SPEED: f32 = 30.0;
/// Pitch the fox snaps to when it faceplants into snow after a missed pounce.
const POUNCE_FACEPLANT_PITCH: f32 = 60.0;
/// The pounce is over once vertical speed, pitch, and ground contact all settle.
const POUNCE_LANDED_Y_SPEED_SQ: f64 = 0.05;
const POUNCE_LEVEL_PITCH: f32 = 15.0;
/// While still airborne, once vertical speed nearly stops the pitch eases to level.
const POUNCE_SETTLE_Y_SPEED_SQ: f64 = 0.03;
const POUNCE_PITCH_SETTLE_LERP: f32 = 0.2;
/// Extra weight given to upward motion when computing the mid-pounce tilt.
const POUNCE_UPWARD_TILT_BIAS: f64 = 6.5;
/// Minimum motion magnitude before the tilt angle is recomputed.
const POUNCE_TILT_EPSILON: f64 = 1.0e-5;
/// Clear-path scan for a pounce: horizontal steps and the head-height range checked.
const PATH_CLEAR_STEPS: i32 = 6;
const PATH_CLEAR_HEIGHT: i32 = 4;

/// Distance squared within which a stalking fox stops and crouches to pounce
/// (vanilla `36.0`, i.e. six blocks).
const STALK_CROUCH_DISTANCE_SQ: f64 = 36.0;
/// Speed a fox creeps toward its prey while stalking (vanilla `1.5`).
const STALK_SPEED: f64 = 1.5;

/// How often, in ticks, a fox checks whether to defend a trusted entity (vanilla
/// passes 10 to the underlying `NearestAttackableTargetGoal`).
const DEFEND_RANDOM_INTERVAL: i32 = reduced_tick_delay(10);

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

/// Vanilla `Mth.rotLerp`: eases `from` toward `to` by `delta`, wrapping the angle.
fn rot_lerp(delta: f32, from: f32, to: f32) -> f32 {
    from + delta * wrap_degrees(to - from)
}

/// Vanilla `Fox.isPathClear`: whether the space between the fox and its target, at
/// head height and above, is all replaceable, so a pounce arc is unobstructed.
fn is_path_clear(mob: &dyn PathfinderMob, target: &SharedEntity) -> bool {
    let Some(world) = mob.level() else {
        return false;
    };
    let fox_pos = mob.position();
    let target_pos = target.position();
    let zdiff = target_pos.z - fox_pos.z;
    let xdiff = target_pos.x - fox_pos.x;
    let slope = zdiff / xdiff;

    for i in 0..PATH_CLEAR_STEPS {
        let fraction = f64::from(i) / f64::from(PATH_CLEAR_STEPS);
        let (x, z) = if slope == 0.0 {
            (xdiff * fraction, 0.0)
        } else {
            let z = zdiff * fraction;
            (z / slope, z)
        };
        for j in 1..PATH_CLEAR_HEIGHT {
            let pos = BlockPos::containing(fox_pos.x + x, fox_pos.y + f64::from(j), fox_pos.z + z);
            if !world.get_block_state(pos).is_replaceable() {
                return false;
            }
        }
    }
    true
}

/// Vanilla `Fox.FoxPounceGoal`: a fully-crouched fox with a clear line to its target
/// leaps at it, striking on contact or faceplanting into snow on a hard miss.
pub(crate) struct FoxPounceGoal;

impl Goal for FoxPounceGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::JUMP
    }

    fn is_interruptable(&self) -> bool {
        false
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(fox) = as_fox(mob) else {
            return false;
        };
        if !fox.is_fully_crouched() {
            return false;
        }
        let Some(target) = Mob::target(fox).filter(|target| target.is_alive()) else {
            return false;
        };
        // Vanilla also skips when the target's motion direction differs from its
        // facing, but getMotionDirection equals getDirection for the fox's prey, so
        // that guard never fires and is dropped.
        let has_clear_path = is_path_clear(mob, &target);
        if !has_clear_path {
            // Vanilla nudges the navigation toward the target here; the fox just
            // stands down from the pounce and leaves the approach to other goals.
            fox.set_crouching(false);
            fox.set_interested(false);
        }
        has_clear_path
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(fox) = as_fox(mob) else {
            return false;
        };
        if !Mob::target(fox).is_some_and(|target| target.is_alive()) {
            return false;
        }
        let y_speed = mob.velocity().y;
        let (_, pitch) = mob.rotation();
        let landed = y_speed * y_speed < POUNCE_LANDED_Y_SPEED_SQ
            && pitch.abs() < POUNCE_LEVEL_PITCH
            && mob.on_ground();
        !landed && !fox.is_faceplanted()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        let Some(fox) = as_fox(mob) else {
            return;
        };
        mob.set_jumping(true);
        fox.set_pouncing(true);
        fox.set_interested(false);
        if let Some(target) = Mob::target(fox) {
            let target_pos = target.position();
            mob.mob_base().controls().lock().look_control.set_look_at(
                target_pos,
                POUNCE_LOOK_Y_SPEED,
                POUNCE_LOOK_X_SPEED,
            );
            let toward = (target_pos - mob.position()).normalize_or_zero();
            let leap = DVec3::new(
                toward.x * POUNCE_LEAP_HORIZONTAL,
                POUNCE_LEAP_VERTICAL,
                toward.z * POUNCE_LEAP_HORIZONTAL,
            );
            mob.set_velocity(mob.velocity() + leap);
        }
        mob.mob_base().navigation().lock().stop();
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        if let Some(fox) = as_fox(mob) {
            fox.set_crouching(false);
            fox.reset_crouch_amount();
            fox.set_interested(false);
            fox.set_pouncing(false);
        }
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let Some(fox) = as_fox(mob) else {
            return;
        };
        let target = Mob::target(fox);
        if let Some(target) = &target {
            mob.mob_base().controls().lock().look_control.set_look_at(
                target.position(),
                POUNCE_LOOK_Y_SPEED,
                POUNCE_LOOK_X_SPEED,
            );
        }

        if !fox.is_faceplanted() {
            let movement = mob.velocity();
            let (yaw, pitch) = mob.rotation();
            if movement.y * movement.y < POUNCE_SETTLE_Y_SPEED_SQ && pitch != 0.0 {
                mob.set_rotation((yaw, rot_lerp(POUNCE_PITCH_SETTLE_LERP, pitch, 0.0)));
            } else {
                let horizontal = movement.x.hypot(movement.z);
                let upward_bias = if mob.is_jumping() && movement.y > 0.0 {
                    POUNCE_UPWARD_TILT_BIAS
                } else {
                    1.0
                };
                let biased_y = movement.y * upward_bias;
                let len = horizontal.hypot(biased_y);
                if len > POUNCE_TILT_EPSILON {
                    let tilt = (-biased_y).signum() * (horizontal / len).acos() * 180.0 / PI;
                    mob.set_rotation((yaw, tilt as f32));
                }
            }
        }

        if let Some(target) = &target
            && mob.position().distance(target.position()) <= POUNCE_HIT_DISTANCE
        {
            if let Some(world) = mob.level() {
                let _ = Mob::do_hurt_target(fox, &world, target);
            }
            return;
        }

        let (yaw, pitch) = mob.rotation();
        if pitch > 0.0
            && mob.on_ground()
            && mob.velocity().y != 0.0
            && let Some(world) = mob.level()
            && world.get_block_state(mob.block_position()).get_block() == &vanilla_blocks::SNOW
        {
            mob.set_rotation((yaw, POUNCE_FACEPLANT_PITCH));
            Mob::set_target(fox, None);
            fox.set_faceplanted(true);
        }
    }
}

/// Vanilla `Fox.STALKABLE_PREY`: what a fox will stalk and pounce.
fn is_stalkable_prey(target: &SharedEntity) -> bool {
    // TODO(fox-prey): vanilla also stalks rabbits; the Rabbit mob is not in the tree yet.
    target.entity_type() == &vanilla_entities::CHICKEN
}

/// Vanilla `Fox.StalkPreyGoal`: a fox creeps toward distant prey, then crouches
/// within pouncing range so the pounce goal can launch.
pub(crate) struct StalkPreyGoal;

impl Goal for StalkPreyGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::LOOK
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(fox) = as_fox(mob) else {
            return false;
        };
        if fox.is_sleeping() {
            return false;
        }
        let Some(target) =
            Mob::target(fox).filter(|target| target.is_alive() && is_stalkable_prey(target))
        else {
            return false;
        };
        mob.position().distance_squared(target.position()) > STALK_CROUCH_DISTANCE_SQ
            && !fox.is_crouching()
            && !fox.is_interested()
            && !mob.is_jumping()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        if let Some(fox) = as_fox(mob) {
            fox.set_sitting(false);
            fox.set_faceplanted(false);
        }
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        let Some(fox) = as_fox(mob) else {
            return;
        };
        let target = Mob::target(fox);
        if let Some(target) = &target
            && is_path_clear(mob, target)
        {
            fox.set_interested(true);
            fox.set_crouching(true);
            mob.mob_base().navigation().lock().stop();
            mob.mob_base().controls().lock().look_control.set_look_at(
                target.position(),
                mob.max_head_y_rot(),
                mob.max_head_x_rot(),
            );
        } else {
            fox.set_interested(false);
            fox.set_crouching(false);
        }
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let Some(fox) = as_fox(mob) else {
            return;
        };
        let Some(target) = Mob::target(fox) else {
            return;
        };
        mob.mob_base().controls().lock().look_control.set_look_at(
            target.position(),
            mob.max_head_y_rot(),
            mob.max_head_x_rot(),
        );
        if mob.position().distance_squared(target.position()) <= STALK_CROUCH_DISTANCE_SQ {
            fox.set_interested(true);
            fox.set_crouching(true);
            mob.mob_base().navigation().lock().stop();
        } else {
            mob.move_to_pos(target.position(), STALK_SPEED);
        }
    }
}

/// Vanilla `Fox.FoxMeleeAttackGoal`: the fox closes on and bites its target. It
/// composes the shared melee goal, adding the fox bite sound and the pose gates
/// that keep a resting or crouched fox from lunging.
pub(crate) struct FoxMeleeAttackGoal {
    inner: MeleeAttackGoal,
}

impl FoxMeleeAttackGoal {
    pub(crate) fn new(speed_modifier: f64) -> Self {
        Self {
            inner: MeleeAttackGoal::new(speed_modifier, true)
                .with_attack_sound(&sound_events::ENTITY_FOX_BITE),
        }
    }
}

impl Goal for FoxMeleeAttackGoal {
    fn controls(&self) -> GoalControls {
        self.inner.controls()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(fox) = as_fox(mob) else {
            return false;
        };
        !fox.is_sitting()
            && !fox.is_sleeping()
            && !fox.is_crouching()
            && !fox.is_faceplanted()
            && self.inner.can_use(mob)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.inner.can_continue_to_use(mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        if let Some(fox) = as_fox(mob) {
            fox.set_interested(false);
        }
        self.inner.start(mob);
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.inner.stop(mob);
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        self.inner.tick(mob);
    }
}

/// Vanilla `Fox.DefendTrustedTargetGoal`: when whatever last hurt a trusted
/// entity is not itself trusted, the fox turns to fight it.
pub(crate) struct DefendTrustedTargetGoal {
    /// The trusted entity's last-hurt-by-mob timestamp this goal last acted on,
    /// so the same hurt event does not retrigger it every tick.
    timestamp: i32,
    pending_attacker: Option<SharedEntity>,
    pending_timestamp: i32,
}

impl DefendTrustedTargetGoal {
    pub(crate) const fn new() -> Self {
        Self {
            timestamp: 0,
            pending_attacker: None,
            pending_timestamp: 0,
        }
    }
}

impl Goal for DefendTrustedTargetGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::TARGET
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(fox) = as_fox(mob) else {
            return false;
        };
        if DEFEND_RANDOM_INTERVAL > 0 && rand::random_range(0..DEFEND_RANDOM_INTERVAL) != 0 {
            return false;
        }
        let Some(world) = mob.level() else {
            return false;
        };

        // Vanilla only inspects the first trusted id that resolves to a living entity.
        let Some(trusted_entity) = fox
            .trusted_ids()
            .into_iter()
            .find_map(|uuid| world.get_entity_by_uuid(&uuid))
        else {
            return false;
        };
        let Some(trusted_living) = trusted_entity.as_living_entity() else {
            return false;
        };

        let timestamp = trusted_living.last_hurt_by_mob_timestamp();
        if timestamp == self.timestamp {
            return false;
        }
        let Some(attacker) = trusted_living.last_hurt_by_mob() else {
            return false;
        };
        let Some(attacker_living) = attacker.as_living_entity() else {
            return false;
        };
        if !attacker.is_alive() || fox.trusts(attacker.uuid()) {
            return false;
        }

        let follow_range = mob
            .attributes()
            .lock()
            .required_value(vanilla_attributes::FOLLOW_RANGE);
        let targeting = TargetingConditions::for_combat().range(follow_range);
        if !targeting.test(
            world.as_ref(),
            Some(fox as &dyn LivingEntity),
            attacker_living,
        ) {
            return false;
        }

        self.pending_timestamp = timestamp;
        self.pending_attacker = Some(attacker);
        true
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(fox) = as_fox(mob) else {
            return false;
        };
        Mob::target(fox).is_some_and(|target| target.is_alive() && !fox.trusts(target.uuid()))
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        let Some(fox) = as_fox(mob) else {
            return;
        };
        let Some(attacker) = self.pending_attacker.take() else {
            return;
        };
        self.timestamp = self.pending_timestamp;
        let _ = Mob::set_target(fox, Some(&attacker));
        fox.play_sound(&sound_events::ENTITY_FOX_AGGRO, 1.0, 1.0);
        fox.set_defending(true);
        fox.set_sleeping(false);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities};

    use super::{is_stalkable_prey, rot_lerp};
    use crate::entity::SharedEntity;
    use crate::entity::entities::{ChickenEntity, PigEntity};

    #[test]
    fn rot_lerp_eases_toward_the_target_angle() {
        // Eases 10 degrees toward 0 by 0.2, landing at 8.
        assert!((rot_lerp(0.2, 10.0, 0.0) - 8.0).abs() < 1.0e-4);
        // Takes the short way across the -180/180 seam: 170 toward -170 is +20.
        assert!((rot_lerp(0.5, 170.0, -170.0) - 180.0).abs() < 1.0e-4);
    }

    #[test]
    fn fox_stalks_chickens_but_not_other_animals() {
        init_vanilla_registry();
        let chicken: SharedEntity = Arc::new(ChickenEntity::new(
            &vanilla_entities::CHICKEN,
            1,
            DVec3::ZERO,
            Weak::new(),
        ));
        let pig: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::ZERO,
            Weak::new(),
        ));

        assert!(is_stalkable_prey(&chicken));
        assert!(!is_stalkable_prey(&pig));
    }
}
