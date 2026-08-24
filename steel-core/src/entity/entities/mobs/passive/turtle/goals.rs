//! Bespoke turtle AI goals.
//!
//! These port the private goal classes nested inside vanilla 26.2 `Turtle`. They
//! read turtle-specific state (`has_egg`, `going_home`, `travel_pos`, the home
//! beach, and the lay-egg counter) that the shared goals cannot express, so they
//! live alongside the entity rather than in the generic goal module.
//!
//! Two vanilla mechanisms are approximated because Steel has no equivalent yet,
//! and both are called out in the pull request for review:
//!
//! * Vanilla turtles swim with a custom `TurtleMoveControl` (water buoyancy and
//!   reduced land speed) and an `AmphibiousPathNavigation`. Steel exposes neither
//!   a per-entity move control nor an amphibious navigator, so the turtle uses the
//!   default control and navigation together with a `WATER` pathfinding malus of
//!   `0.0`. Water motion is therefore not pixel-perfect.
//! * `TurtleTravelGoal` in vanilla rejects a swim target whose destination chunks
//!   are not loaded. Steel has no loaded-area query available to a goal, so that
//!   guard is omitted; an unreachable target simply leaves the navigation idle and
//!   the goal stops through `can_continue_to_use`.

use std::f64::consts::{FRAC_PI_2, PI};

use glam::DVec3;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, IntProperty};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_game_rules::MOB_DROPS;
use steel_registry::{sound_events, vanilla_blocks, vanilla_damage_type_tags, vanilla_game_events};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, Downcast as _};

use super::TurtleEntity;
use crate::entity::ai::goal::{
    Goal, GoalControls, MoveToBlockGoal, RandomStrollGoal, default_random_pos,
    default_random_pos_towards, look_for_water, reduced_tick_delay,
};
use crate::entity::ai::targeting::TargetingConditions;
use crate::entity::entities::ExperienceOrbEntity;
use crate::entity::{AgeableMob, Animal, PathfinderMob, SharedEntity};
use crate::world::game_event::GameEventContext;

/// Number of eggs stored in a placed turtle egg cluster (`1..=4`).
const EGGS: &IntProperty = &BlockStateProperties::EGGS;
/// Vanilla `TurtleBreedGoal` partner search radius.
const PARTNER_SEARCH_RANGE: f64 = 8.0;
/// Vanilla `BreedGoal.SPAWN_CHILD_DISTANCE` squared: how close parents must be.
const BREED_DISTANCE_SQR: f64 = 9.0;
/// Vanilla `BreedGoal` love timer length before an egg is granted.
const BREED_TIME: i32 = 60;

/// Returns the concrete turtle behind a pathfinder mob, if this mob is a turtle.
fn as_turtle(mob: &dyn PathfinderMob) -> Option<&TurtleEntity> {
    mob.downcast_ref::<TurtleEntity>()
}

/// Vanilla `BlockPos.closerToCenterThan`: squared distance from the block center.
fn closer_to_center_than(block: BlockPos, position: DVec3, distance: f64) -> bool {
    let (x, y, z) = block.get_center();
    DVec3::new(x, y, z).distance_squared(position) < distance * distance
}

/// Vanilla `Vec3.atBottomCenterOf`.
fn bottom_center(block: BlockPos) -> DVec3 {
    let (x, y, z) = block.get_bottom_center();
    DVec3::new(x, y, z)
}

/// Vanilla `Turtle.TurtlePanicGoal`: always try to reach water when panicking,
/// not only while on fire, then fall back to a random escape position.
pub(super) struct TurtlePanicGoal {
    wanted_position: Option<DVec3>,
    speed_modifier: f64,
    is_running: bool,
}

impl TurtlePanicGoal {
    pub(super) const fn new(speed_modifier: f64) -> Self {
        Self {
            wanted_position: None,
            speed_modifier,
            is_running: false,
        }
    }

    fn should_panic(mob: &dyn PathfinderMob) -> bool {
        mob.last_damage_source()
            .is_some_and(|source| source.is(&vanilla_damage_type_tags::DamageTypeTag::PANIC_CAUSES))
    }
}

impl Goal for TurtlePanicGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE
    }

    fn is_panic_goal(&self) -> bool {
        true
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        if !Self::should_panic(mob) {
            return false;
        }

        if let Some(water) = look_for_water(mob, 7) {
            self.wanted_position = Some(DVec3::new(
                f64::from(water.x()),
                f64::from(water.y()),
                f64::from(water.z()),
            ));
            return true;
        }

        let Some(position) = default_random_pos(mob, 5, 4) else {
            return false;
        };
        self.wanted_position = Some(position);
        true
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        !mob.mob_base().navigation().lock().is_done()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        if let Some(wanted_position) = self.wanted_position {
            mob.move_to_pos(wanted_position, self.speed_modifier);
        }
        self.is_running = true;
    }

    fn stop(&mut self, _mob: &dyn PathfinderMob) {
        self.is_running = false;
    }
}

/// Vanilla `Turtle.TurtleBreedGoal`: breeding gives the mother an egg to lay
/// instead of spawning a baby, and both parents grow back to adulthood.
///
/// Steel's shared `BreedGoal` spawns the child directly in its tick with no
/// override point, so the partner search and love timer are reimplemented here to
/// substitute the egg-granting step. A `breed()`-style hook on the shared goal
/// would remove this duplication; that is raised for discussion on the PR.
pub(super) struct TurtleBreedGoal {
    partner: Option<SharedEntity>,
    love_time: i32,
    speed_modifier: f64,
}

impl TurtleBreedGoal {
    pub(super) const fn new(speed_modifier: f64) -> Self {
        Self {
            partner: None,
            love_time: 0,
            speed_modifier,
        }
    }

    fn find_partner(mob: &dyn PathfinderMob, animal: &dyn Animal) -> Option<SharedEntity> {
        let world = mob.level()?;
        let search_box = mob.bounding_box().inflate(PARTNER_SEARCH_RANGE);
        let partner_targeting = TargetingConditions::for_non_combat()
            .range(PARTNER_SEARCH_RANGE)
            .ignore_line_of_sight();

        world.nearest_entity_in_aabb_matching(&search_box, mob.position(), |entity| {
            let Some(candidate) = entity.as_animal() else {
                return false;
            };
            if !partner_targeting.test(world.as_ref(), Some(mob), candidate) {
                return false;
            }
            if !animal.can_mate(candidate) {
                return false;
            }

            !entity
                .as_pathfinder_mob()
                .is_some_and(PathfinderMob::is_panicking)
        })
    }

    /// Vanilla `TurtleBreedGoal.breed`: mark the mother as carrying an egg, age
    /// both parents back to adulthood, clear love mode, and drop breeding XP.
    fn breed(mob: &dyn PathfinderMob, turtle: &TurtleEntity, partner_animal: &dyn Animal) {
        let Some(world) = mob.level() else {
            return;
        };

        turtle.set_has_egg(true);
        turtle.set_age(6000);
        partner_animal.set_age(6000);
        turtle.reset_love();
        partner_animal.reset_love();

        if world.get_game_rule(&MOB_DROPS) {
            let xp = rand::random_range(0..7) + 1;
            ExperienceOrbEntity::award(&world, mob.position(), xp);
        }
    }
}

impl Goal for TurtleBreedGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::LOOK
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(turtle) = as_turtle(mob) else {
            return false;
        };
        if turtle.has_egg() || !turtle.is_in_love() {
            return false;
        }

        self.partner = Self::find_partner(mob, turtle);
        self.partner.is_some()
    }

    fn can_continue_to_use(&mut self, _mob: &dyn PathfinderMob) -> bool {
        let Some(partner) = &self.partner else {
            return false;
        };
        if !partner.is_alive() || self.love_time >= BREED_TIME {
            return false;
        }
        if partner
            .as_pathfinder_mob()
            .is_some_and(PathfinderMob::is_panicking)
        {
            return false;
        }

        partner.as_animal().is_some_and(Animal::is_in_love)
    }

    fn stop(&mut self, _mob: &dyn PathfinderMob) {
        self.partner = None;
        self.love_time = 0;
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let Some(partner) = &self.partner else {
            return;
        };
        let Some(turtle) = as_turtle(mob) else {
            return;
        };
        let Some(partner_animal) = partner.as_animal() else {
            return;
        };

        let partner_position = partner.position();
        mob.mob_base().controls().lock().look_control.set_look_at(
            DVec3::new(partner_position.x, partner.get_eye_y(), partner_position.z),
            10.0,
            mob.max_head_x_rot(),
        );
        mob.move_to_pos(partner_position, self.speed_modifier);

        self.love_time += 1;
        if self.love_time < reduced_tick_delay(BREED_TIME)
            || mob.position().distance_squared(partner_position) >= BREED_DISTANCE_SQR
        {
            return;
        }

        Self::breed(mob, turtle, partner_animal);
    }
}

/// Vanilla `Turtle.TurtleLayEggGoal`: walk to sand near home and, after a delay,
/// place a turtle egg cluster and clear the carried egg.
pub(super) struct TurtleLayEggGoal {
    inner: MoveToBlockGoal,
}

impl TurtleLayEggGoal {
    pub(super) fn new(speed_modifier: f64) -> Self {
        Self {
            inner: MoveToBlockGoal::new(speed_modifier, 16, |level, pos| {
                level.get_block_state(pos.above()).is_air()
                    && level
                        .get_block_state(pos)
                        .get_block()
                        .has_tag(&BlockTag::SAND)
            }),
        }
    }

    fn within_home(turtle: &TurtleEntity, mob: &dyn PathfinderMob) -> bool {
        closer_to_center_than(turtle.home_pos(), mob.position(), 9.0)
    }
}

impl Goal for TurtleLayEggGoal {
    fn controls(&self) -> GoalControls {
        self.inner.controls()
    }

    fn requires_update_every_tick(&self) -> bool {
        self.inner.requires_update_every_tick()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(turtle) = as_turtle(mob) else {
            return false;
        };
        turtle.has_egg() && Self::within_home(turtle, mob) && self.inner.can_use(mob)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(turtle) = as_turtle(mob) else {
            return false;
        };
        self.inner.can_continue_to_use(mob) && turtle.has_egg() && Self::within_home(turtle, mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.inner.start(mob);
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.inner.stop(mob);
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        self.inner.tick(mob);

        let Some(turtle) = as_turtle(mob) else {
            return;
        };
        if mob.is_in_water() || !self.inner.is_reached_target() {
            return;
        }

        if turtle.lay_egg_counter() < 1 {
            turtle.set_laying_egg(true);
        } else if turtle.lay_egg_counter() > 200 {
            self.place_egg(mob, turtle);
        }

        if turtle.is_laying_egg() {
            turtle.increment_lay_egg_counter();
        }
    }
}

impl TurtleLayEggGoal {
    fn place_egg(&self, mob: &dyn PathfinderMob, turtle: &TurtleEntity) {
        let Some(world) = mob.level() else {
            return;
        };

        let egg_pos = self.inner.block_pos().above();
        let count = rand::random_range(1..=4u8);
        let egg_state = vanilla_blocks::TURTLE_EGG
            .default_state()
            .set_value(EGGS, count);

        world.play_sound(
            &sound_events::ENTITY_TURTLE_LAY_EGG,
            SoundSource::Blocks,
            mob.block_position(),
            0.3,
            0.9 + rand::random::<f32>() * 0.2,
            None,
        );
        world.set_block(egg_pos, egg_state, UpdateFlags::UPDATE_ALL);
        world.game_event(
            &vanilla_game_events::BLOCK_PLACE,
            egg_pos,
            &GameEventContext::new(Some(turtle), Some(egg_state)),
        );

        turtle.set_has_egg(false);
        turtle.set_laying_egg(false);
        turtle.set_in_love_time(600);
    }
}

/// Vanilla `Turtle.TurtleGoToWaterGoal`: leave land for the nearest water block.
pub(super) struct TurtleGoToWaterGoal {
    inner: MoveToBlockGoal,
}

impl TurtleGoToWaterGoal {
    pub(super) fn new(speed_modifier: f64) -> Self {
        Self {
            inner: MoveToBlockGoal::new(speed_modifier, 24, |level, pos| {
                level.get_block_state(pos).get_block() == &vanilla_blocks::WATER
            })
            .with_vertical_search_start(-1)
            .with_recalculate_path_interval(160),
        }
    }
}

impl Goal for TurtleGoToWaterGoal {
    fn controls(&self) -> GoalControls {
        self.inner.controls()
    }

    fn requires_update_every_tick(&self) -> bool {
        self.inner.requires_update_every_tick()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(turtle) = as_turtle(mob) else {
            return false;
        };
        if AgeableMob::is_baby(turtle) && !mob.is_in_water() {
            return self.inner.can_use(mob);
        }

        !turtle.going_home() && !mob.is_in_water() && !turtle.has_egg() && self.inner.can_use(mob)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        // Vanilla drops the shared goal's lower `try_ticks` bound here; that bound
        // only matters after long dwell at a reached target, which cannot happen
        // while the turtle is still out of water, so the behavior is equivalent.
        !mob.is_in_water() && self.inner.can_continue_to_use(mob)
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

/// Vanilla `Turtle.TurtleGoHomeGoal`: head back toward the home beach, always
/// when carrying an egg and otherwise on a rare timer when far from home.
pub(super) struct TurtleGoHomeGoal {
    speed_modifier: f64,
    stuck: bool,
    close_to_home_try_ticks: i32,
}

impl TurtleGoHomeGoal {
    pub(super) const fn new(speed_modifier: f64) -> Self {
        Self {
            speed_modifier,
            stuck: false,
            close_to_home_try_ticks: 0,
        }
    }
}

impl Goal for TurtleGoHomeGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(turtle) = as_turtle(mob) else {
            return false;
        };
        if AgeableMob::is_baby(turtle) {
            return false;
        }
        if turtle.has_egg() {
            return true;
        }

        rand::random_range(0..reduced_tick_delay(700)) == 0
            && !closer_to_center_than(turtle.home_pos(), mob.position(), 64.0)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(turtle) = as_turtle(mob) else {
            return false;
        };
        !closer_to_center_than(turtle.home_pos(), mob.position(), 7.0)
            && !self.stuck
            && self.close_to_home_try_ticks <= reduced_tick_delay(600)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        if let Some(turtle) = as_turtle(mob) {
            turtle.set_going_home(true);
        }
        self.stuck = false;
        self.close_to_home_try_ticks = 0;
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        if let Some(turtle) = as_turtle(mob) {
            turtle.set_going_home(false);
        }
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let Some(turtle) = as_turtle(mob) else {
            return;
        };
        let home_pos = turtle.home_pos();
        let close_to_home = closer_to_center_than(home_pos, mob.position(), 16.0);
        if close_to_home {
            self.close_to_home_try_ticks += 1;
        }

        if !mob.mob_base().navigation().lock().is_done() {
            return;
        }

        let home_vec = bottom_center(home_pos);
        let mut next = default_random_pos_towards(mob, 16, 3, home_vec, PI / 10.0)
            .or_else(|| default_random_pos_towards(mob, 8, 7, home_vec, FRAC_PI_2));

        if let Some(candidate) = next
            && !close_to_home
            && mob.level().is_some_and(|world| {
                world
                    .get_block_state(BlockPos::containing(candidate.x, candidate.y, candidate.z))
                    .get_block()
                    != &vanilla_blocks::WATER
            })
        {
            next = default_random_pos_towards(mob, 16, 5, home_vec, FRAC_PI_2);
        }

        let Some(next) = next else {
            self.stuck = true;
            return;
        };
        mob.move_to_pos(next, self.speed_modifier);
    }
}

/// Vanilla `Turtle.TurtleTravelGoal`: pick a far swim target and wander to it.
pub(super) struct TurtleTravelGoal {
    speed_modifier: f64,
    stuck: bool,
}

impl TurtleTravelGoal {
    pub(super) const fn new(speed_modifier: f64) -> Self {
        Self {
            speed_modifier,
            stuck: false,
        }
    }
}

impl Goal for TurtleTravelGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(turtle) = as_turtle(mob) else {
            return false;
        };
        !turtle.going_home() && !turtle.has_egg() && mob.is_in_water()
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(turtle) = as_turtle(mob) else {
            return false;
        };
        !mob.mob_base().navigation().lock().is_done()
            && !self.stuck
            && !turtle.going_home()
            && !turtle.is_in_love()
            && !turtle.has_egg()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        let Some(turtle) = as_turtle(mob) else {
            return;
        };
        let Some(world) = mob.level() else {
            return;
        };

        let position = mob.position();
        let xt = f64::from(rand::random_range(0..1025) - 512);
        let mut yt = f64::from(rand::random_range(0..9) - 4);
        let zt = f64::from(rand::random_range(0..1025) - 512);
        if yt + position.y > f64::from(world.sea_level - 1) {
            yt = 0.0;
        }

        turtle.set_travel_pos(Some(BlockPos::containing(
            xt + position.x,
            yt + position.y,
            zt + position.z,
        )));
        self.stuck = false;
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        if let Some(turtle) = as_turtle(mob) {
            turtle.set_travel_pos(None);
        }
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let Some(turtle) = as_turtle(mob) else {
            return;
        };
        let Some(travel_pos) = turtle.travel_pos() else {
            self.stuck = true;
            return;
        };

        if !mob.mob_base().navigation().lock().is_done() {
            return;
        }

        let target = bottom_center(travel_pos);
        let next = default_random_pos_towards(mob, 16, 3, target, PI / 10.0)
            .or_else(|| default_random_pos_towards(mob, 8, 7, target, FRAC_PI_2));

        let Some(next) = next else {
            self.stuck = true;
            return;
        };
        mob.move_to_pos(next, self.speed_modifier);
    }
}

/// Vanilla `Turtle.TurtleRandomStrollGoal`: stroll only on land, and never while
/// heading home or carrying an egg.
pub(super) struct TurtleRandomStrollGoal {
    inner: RandomStrollGoal,
}

impl TurtleRandomStrollGoal {
    pub(super) const fn new(speed_modifier: f64, interval: i32) -> Self {
        Self {
            inner: RandomStrollGoal::with_interval(speed_modifier, interval),
        }
    }
}

impl Goal for TurtleRandomStrollGoal {
    fn controls(&self) -> GoalControls {
        self.inner.controls()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(turtle) = as_turtle(mob) else {
            return false;
        };
        !mob.is_in_water() && !turtle.going_home() && !turtle.has_egg() && self.inner.can_use(mob)
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
}
