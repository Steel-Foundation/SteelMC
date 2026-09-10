//! Bespoke fox behaviour goals.

use std::f64::consts::TAU;
use std::ops::RangeInclusive;
use std::sync::Arc;

use glam::DVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty, IntProperty};
use steel_registry::item_stack::ItemStack;
use steel_registry::{
    sound_events, vanilla_blocks, vanilla_game_events, vanilla_game_rules, vanilla_items,
};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Downcast as _};

use super::FoxEntity;
use crate::behavior::blocks::vegetation::CaveVinesBlock;
use crate::entity::ai::goal::{
    BreedGoal, FloatGoal, FollowParentGoal, Goal, GoalControls, LookAtPlayerGoal, MoveToBlockGoal,
    PanicGoal, reduced_tick_delay,
};
use crate::entity::entities::objects::items::ItemEntity;
use crate::entity::{Entity, LivingEntity, Mob, MobBase, PathfinderMob};
use crate::inventory::equipment::EquipmentSlot;
use crate::world::game_event::GameEventContext;
use crate::world::{LevelReader as _, World};

const SEARCH_RANGE: f64 = 8.0;
const SEARCH_CHECK_TICKS: i32 = 10;
const SEARCH_SPEED: f64 = 1.2;

const PERCH_CHANCE: f32 = 0.02;
const PERCH_MIN_LOOKS: i32 = 2;
const PERCH_EXTRA_LOOKS: i32 = 3;
const PERCH_MIN_LOOK_TICKS: i32 = 80;
const PERCH_EXTRA_LOOK_TICKS: i32 = 20;

pub(super) const FOX_FLOAT_WATER_DEPTH: f64 = 0.25;
const BERRY_SEARCH_RANGE: i32 = 12;
const BERRY_VERTICAL_SEARCH_RANGE: i32 = 1;
const BERRY_ACCEPTED_DISTANCE: f64 = 2.0;
const BERRY_RECALCULATE_INTERVAL: i32 = 100;
pub(super) const BERRY_WAIT_TICKS: i32 = 40;
const BERRY_SNIFF_CHANCE: f32 = 0.05;
const BERRIES_PER_PICK: RangeInclusive<i32> = 1..=2;
const SWEET_BERRY_RIPE_AGE: u8 = 2;
const SWEET_BERRY_MAX_AGE: u8 = 3;
const SWEET_BERRY_PICKED_AGE: u8 = 1;
const SWEET_BERRY_AGE: &IntProperty = &BlockStateProperties::AGE_3;
const BERRIES: &BoolProperty = &BlockStateProperties::BERRIES;

fn has_glow_berries(state: BlockStateId) -> bool {
    state.try_get_value(BERRIES).unwrap_or(false)
}

fn is_ripe_berry_block(state: BlockStateId) -> bool {
    (state.get_block() == &vanilla_blocks::SWEET_BERRY_BUSH
        && state.get_value(SWEET_BERRY_AGE) >= SWEET_BERRY_RIPE_AGE)
        || has_glow_berries(state)
}

/// Randomized delay, in ticks, before a fox may fall asleep (vanilla 140).
const SLEEP_WAIT_TICKS: i32 = reduced_tick_delay(140);

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

pub(crate) struct FoxEatBerriesGoal {
    inner: MoveToBlockGoal,
    ticks_waited: i32,
}

impl FoxEatBerriesGoal {
    pub(crate) fn new(speed_modifier: f64) -> Self {
        Self {
            inner: MoveToBlockGoal::with_vertical_search_range(
                speed_modifier,
                BERRY_SEARCH_RANGE,
                BERRY_VERTICAL_SEARCH_RANGE,
                |level, pos| is_ripe_berry_block(level.get_block_state(pos)),
            )
            .with_accepted_distance(BERRY_ACCEPTED_DISTANCE)
            .with_recalculate_path_interval(BERRY_RECALCULATE_INTERVAL),
            ticks_waited: 0,
        }
    }

    /// Takes the berries, unless `mobGriefing` is off.
    fn on_reached_target(&self, mob: &dyn PathfinderMob) {
        let (Some(fox), Some(world)) = (as_fox(mob), mob.level()) else {
            return;
        };
        if !world.get_game_rule(&vanilla_game_rules::MOB_GRIEFING) {
            return;
        }

        let pos = self.inner.block_pos();
        let state = world.get_block_state(pos);
        if state.get_block() == &vanilla_blocks::SWEET_BERRY_BUSH {
            Self::pick_sweet_berries(fox, &world, pos, state);
        } else if has_glow_berries(state) {
            CaveVinesBlock::use_block(fox, state, &world, pos);
        }
    }

    /// One berry goes in the mouth if it is free, the rest drop by the bush.
    fn pick_sweet_berries(fox: &FoxEntity, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        let age = state.get_value(SWEET_BERRY_AGE);
        let mut count =
            rand::random_range(BERRIES_PER_PICK) + i32::from(age == SWEET_BERRY_MAX_AGE);

        let mut mouth_is_empty = false;
        fox.with_equipment_slot(EquipmentSlot::MainHand, &mut |item_stack| {
            mouth_is_empty = item_stack.is_empty();
        });
        if mouth_is_empty {
            fox.living_base().equipment().lock().set(
                EquipmentSlot::MainHand,
                ItemStack::new(&vanilla_items::SWEET_BERRIES),
            );
            count -= 1;
        }

        if count > 0 {
            world.pop_resource(
                pos,
                ItemStack::with_count(&vanilla_items::SWEET_BERRIES, count),
            );
        }

        fox.play_sound(&sound_events::BLOCK_SWEET_BERRY_BUSH_PICK_BERRIES, 1.0, 1.0);
        let picked = state.set_value(SWEET_BERRY_AGE, SWEET_BERRY_PICKED_AGE);
        world.set_block(pos, picked, UpdateFlags::UPDATE_CLIENTS);
        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(Some(fox), None),
        );
    }
}

impl Goal for FoxEatBerriesGoal {
    fn controls(&self) -> GoalControls {
        self.inner.controls()
    }

    fn requires_update_every_tick(&self) -> bool {
        self.inner.requires_update_every_tick()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        !as_fox(mob).is_some_and(FoxEntity::is_sleeping) && self.inner.can_use(mob)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.inner.can_continue_to_use(mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.ticks_waited = 0;
        if let Some(fox) = as_fox(mob) {
            fox.set_sitting(false);
        }
        self.inner.start(mob);
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.inner.stop(mob);
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        if self.inner.is_reached_target() {
            if self.ticks_waited >= BERRY_WAIT_TICKS {
                self.on_reached_target(mob);
            } else {
                self.ticks_waited += 1;
            }
        } else if rand::random::<f32>() < BERRY_SNIFF_CHANCE
            && let Some(fox) = as_fox(mob)
        {
            fox.play_sound(&sound_events::ENTITY_FOX_SNIFF, 1.0, 1.0);
        }

        self.inner.tick(mob);
    }
}
