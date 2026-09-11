//! Bespoke fox behaviour goals.

use std::f64::consts::TAU;
use std::sync::Arc;

use glam::DVec3;
use steel_math::{RAD_TO_DEG_F64, rot_lerp};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::{sound_events, vanilla_blocks, vanilla_entities};
use steel_utils::{BlockPos, Downcast as _};

use super::FoxEntity;
use crate::entity::ai::goal::{
    BreedGoal, FloatGoal, FollowParentGoal, Goal, GoalControls, LookAtPlayerGoal, MeleeAttackGoal,
    NearestAttackableTargetGoal, PanicGoal, reduced_tick_delay,
};
use crate::entity::entities::objects::items::ItemEntity;
use crate::entity::{Entity, LivingEntity, Mob, MobBase, PathfinderMob, SharedEntity};
use crate::inventory::equipment::EquipmentSlot;
use crate::world::World;

const SEARCH_RANGE: f64 = 8.0;
const SEARCH_CHECK_TICKS: i32 = 10;
const SEARCH_SPEED: f64 = 1.2;

const PERCH_CHANCE: f32 = 0.02;
const PERCH_MIN_LOOKS: i32 = 2;
const PERCH_EXTRA_LOOKS: i32 = 3;
const PERCH_MIN_LOOK_TICKS: i32 = 80;
const PERCH_EXTRA_LOOK_TICKS: i32 = 20;

pub(super) const FOX_FLOAT_WATER_DEPTH: f64 = 0.25;
const POUNCE_LEAP_HORIZONTAL: f64 = 0.8;
const POUNCE_LEAP_VERTICAL: f64 = 0.9;
const POUNCE_HIT_DISTANCE: f64 = 2.0;
const POUNCE_LOOK_Y_SPEED: f32 = 60.0;
const POUNCE_LOOK_X_SPEED: f32 = 30.0;
const POUNCE_FACEPLANT_PITCH: f32 = 60.0;
const POUNCE_LANDED_Y_SPEED_SQ: f64 = 0.05;
const POUNCE_LEVEL_PITCH: f32 = 15.0;
const POUNCE_SETTLE_Y_SPEED_SQ: f64 = 0.03;
const POUNCE_PITCH_SETTLE_LERP: f32 = 0.2;
const POUNCE_UPWARD_TILT_BIAS: f64 = 6.5;
const POUNCE_TILT_EPSILON: f64 = 1.0e-5;
const PATH_CLEAR_STEPS: i32 = 6;
const PATH_CLEAR_HEIGHT: i32 = 4;
const SLEEP_WAIT_TICKS: i32 = reduced_tick_delay(140);

const STALK_CROUCH_DISTANCE_SQ: f64 = 36.0;
const STALK_SPEED: f64 = 1.5;

const DEFEND_TARGET_INTERVAL: i32 = 10;
const DEFEND_ATTACKER_GRUDGE_TICKS: i32 = 600;

fn as_fox(mob: &dyn PathfinderMob) -> Option<&FoxEntity> {
    mob.downcast_ref::<FoxEntity>()
}

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

/// An idle fox sleeps under cover during the day.
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

/// Swims in shallower water than most mobs, dropping other goals.
pub(crate) struct FoxFloatGoal {
    inner: FloatGoal,
}

impl FoxFloatGoal {
    pub(crate) fn new(mob_base: &MobBase) -> Self {
        Self {
            inner: FloatGoal::new(mob_base),
        }
    }
}

impl Goal for FoxFloatGoal {
    fn controls(&self) -> GoalControls {
        self.inner.controls()
    }

    fn requires_update_every_tick(&self) -> bool {
        self.inner.requires_update_every_tick()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        (mob.is_in_water() && mob.fluid_contact().water_height() > FOX_FLOAT_WATER_DEPTH)
            || mob.is_in_lava()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.inner.start(mob);
        if let Some(fox) = as_fox(mob) {
            fox.clear_states();
        }
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.inner.stop(mob);
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        self.inner.tick(mob);
    }
}

/// A fox standing up for something it trusts holds its ground instead of bolting.
pub(crate) struct FoxPanicGoal {
    inner: PanicGoal,
}

impl FoxPanicGoal {
    pub(crate) const fn new(speed_modifier: f64) -> Self {
        Self {
            inner: PanicGoal::new(speed_modifier),
        }
    }
}

impl Goal for FoxPanicGoal {
    fn controls(&self) -> GoalControls {
        self.inner.controls()
    }

    fn is_panic_goal(&self) -> bool {
        self.inner.is_panic_goal()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        !as_fox(mob).is_some_and(FoxEntity::is_defending) && self.inner.can_use(mob)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.inner.can_continue_to_use(mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.inner.start(mob);
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.inner.stop(mob);
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        self.inner.tick(mob);
    }
}

/// Both foxes settle down before courting.
pub(crate) struct FoxBreedGoal {
    inner: BreedGoal,
}

impl FoxBreedGoal {
    pub(crate) const fn new(speed_modifier: f64) -> Self {
        Self {
            inner: BreedGoal::new(speed_modifier),
        }
    }
}

impl Goal for FoxBreedGoal {
    fn controls(&self) -> GoalControls {
        self.inner.controls()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.inner.can_use(mob)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.inner.can_continue_to_use(mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        if let Some(fox) = as_fox(mob) {
            fox.clear_states();
        }
        if let Some(partner) = self
            .inner
            .partner()
            .and_then(|partner| partner.downcast_ref::<FoxEntity>())
        {
            partner.clear_states();
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

/// A defending kit stays put instead of trailing its parent.
pub(crate) struct FoxFollowParentGoal {
    inner: FollowParentGoal,
}

impl FoxFollowParentGoal {
    pub(crate) const fn new(speed_modifier: f64) -> Self {
        Self {
            inner: FollowParentGoal::new(speed_modifier),
        }
    }
}

impl Goal for FoxFollowParentGoal {
    fn controls(&self) -> GoalControls {
        self.inner.controls()
    }

    fn requires_update_every_tick(&self) -> bool {
        self.inner.requires_update_every_tick()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        !as_fox(mob).is_some_and(FoxEntity::is_defending) && self.inner.can_use(mob)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        !as_fox(mob).is_some_and(FoxEntity::is_defending) && self.inner.can_continue_to_use(mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        if let Some(fox) = as_fox(mob) {
            fox.clear_states();
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

/// Skipped while interested or faceplanted.
pub(crate) struct FoxLookAtPlayerGoal {
    inner: LookAtPlayerGoal,
}

impl FoxLookAtPlayerGoal {
    pub(crate) fn new(look_distance: f64) -> Self {
        Self {
            inner: LookAtPlayerGoal::new(look_distance),
        }
    }

    fn is_distracted(mob: &dyn PathfinderMob) -> bool {
        as_fox(mob).is_some_and(|fox| fox.is_faceplanted() || fox.is_interested())
    }
}

impl Goal for FoxLookAtPlayerGoal {
    fn controls(&self) -> GoalControls {
        self.inner.controls()
    }

    fn requires_update_every_tick(&self) -> bool {
        self.inner.requires_update_every_tick()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.inner.can_use(mob) && !Self::is_distracted(mob)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.inner.can_continue_to_use(mob) && !Self::is_distracted(mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.inner.start(mob);
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.inner.stop(mob);
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        self.inner.tick(mob);
    }
}

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

/// Leaps at the target, faceplanting into snow on a hard miss.
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
        let has_clear_path = is_path_clear(mob, &target);
        if !has_clear_path {
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
                    let tilt = (-biased_y).signum() * (horizontal / len).acos() * RAD_TO_DEG_F64;
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

fn is_stalkable_prey(target: &SharedEntity) -> bool {
    // TODO(fox-prey): vanilla also stalks rabbits; the Rabbit mob is not in the tree yet.
    target.entity_type() == &vanilla_entities::CHICKEN
}

/// A fox creeps toward distant prey, then crouches within pouncing range.
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

/// The fox closes on and bites its target, but not while resting or crouched.
pub(crate) struct FoxMeleeAttackGoal {
    inner: MeleeAttackGoal,
}

impl FoxMeleeAttackGoal {
    pub(crate) const fn new(speed_modifier: f64) -> Self {
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

fn recently_aggressive(attacker: &dyn LivingEntity) -> bool {
    attacker.last_hurt_mob().is_some()
        && attacker.last_hurt_mob_timestamp() < attacker.tick_count() + DEFEND_ATTACKER_GRUDGE_TICKS
}

pub(crate) struct DefendTrustedTargetGoal {
    inner: NearestAttackableTargetGoal,
    /// The last-hurt-by timestamp this goal already acted on.
    timestamp: i32,
    pending_timestamp: i32,
}

impl DefendTrustedTargetGoal {
    pub(crate) fn new() -> Self {
        Self {
            inner: NearestAttackableTargetGoal::new_with_interval(
                DEFEND_TARGET_INTERVAL,
                false,
                false,
                |attacker, _| recently_aggressive(attacker),
            ),
            timestamp: 0,
            pending_timestamp: 0,
        }
    }
}

impl Goal for DefendTrustedTargetGoal {
    fn controls(&self) -> GoalControls {
        self.inner.controls()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(fox) = as_fox(mob) else {
            return false;
        };
        let interval = self.inner.random_interval();
        if interval > 0 && rand::random_range(0..interval) != 0 {
            return false;
        }
        let Some(world) = mob.level() else {
            return false;
        };

        let Some(trusted) = fox
            .trusted_ids()
            .into_iter()
            .find_map(|uuid| world.get_entity_by_uuid(&uuid))
        else {
            return false;
        };
        let Some(trusted_living) = trusted.as_living_entity() else {
            return false;
        };

        let timestamp = trusted_living.last_hurt_by_mob_timestamp();
        if timestamp == self.timestamp {
            return false;
        }
        let Some(attacker) = trusted_living.last_hurt_by_mob() else {
            return false;
        };
        if fox.trusts(attacker.uuid()) {
            return false;
        }
        if !self.inner.can_attack(mob, attacker.as_living_entity()) {
            return false;
        }

        self.inner.set_target(Some(attacker));
        self.pending_timestamp = timestamp;
        true
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.inner.can_continue_to_use(mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        let Some(fox) = as_fox(mob) else {
            return;
        };
        self.timestamp = self.pending_timestamp;
        fox.play_sound(&sound_events::ENTITY_FOX_AGGRO, 1.0, 1.0);
        fox.set_defending(true);
        fox.set_sleeping(false);
        self.inner.start(mob);
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.inner.stop(mob);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities};

    use super::is_stalkable_prey;
    use crate::entity::SharedEntity;
    use crate::entity::entities::{ChickenEntity, PigEntity};

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
