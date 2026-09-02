//! Turtle land goals: returning to the home beach, and strolling on land.

use std::f64::consts::{FRAC_PI_2, PI};

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::vanilla_blocks;
use steel_utils::BlockPos;

use super::{
    TOWARD_TARGET_FALLBACK_H, TOWARD_TARGET_FALLBACK_V, TOWARD_TARGET_H, TOWARD_TARGET_V,
    as_turtle, bottom_center, closer_to_center_than,
};
use crate::entity::ai::goal::{
    Goal, GoalControls, RandomStrollGoal, default_random_pos_towards, reduced_tick_delay,
};
use crate::entity::{AgeableMob, PathfinderMob};

/// Vanilla `TurtleGoHomeGoal`: with no egg to lay, roll a 1-in-`reducedTickDelay`
/// of this each tick to decide whether to head home.
const GO_HOME_CHECK_INTERVAL: i32 = 700;
/// Vanilla `TurtleGoHomeGoal`: only start heading home when at least this far from it.
const GO_HOME_MIN_DISTANCE: f64 = 64.0;
/// Vanilla `TurtleGoHomeGoal`: home counts as reached within this distance.
const HOME_REACHED_DISTANCE: f64 = 7.0;
/// Vanilla `TurtleGoHomeGoal.GIVE_UP_TICKS`: stop trying after this long lingering near home.
const GIVE_UP_TICKS: i32 = 600;
/// Vanilla `TurtleGoHomeGoal`: near enough home to count down the give-up timer.
const NEAR_HOME_DISTANCE: f64 = 16.0;
/// Vanilla `TurtleGoHomeGoal`: vertical radius for the last attempt that avoids
/// stepping into water (horizontal radius stays [`TOWARD_TARGET_H`]).
const AVOID_WATER_V: i32 = 5;

/// Vanilla `Turtle.TurtleGoHomeGoal`: head back toward the home beach, always
/// when carrying an egg and otherwise on a rare timer when far from home.
pub(crate) struct TurtleGoHomeGoal {
    speed_modifier: f64,
    stuck: bool,
    close_to_home_try_ticks: i32,
}

impl TurtleGoHomeGoal {
    pub(crate) const fn new(speed_modifier: f64) -> Self {
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

        rand::random_range(0..reduced_tick_delay(GO_HOME_CHECK_INTERVAL)) == 0
            && !closer_to_center_than(turtle.home_pos(), mob.position(), GO_HOME_MIN_DISTANCE)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(turtle) = as_turtle(mob) else {
            return false;
        };
        !closer_to_center_than(turtle.home_pos(), mob.position(), HOME_REACHED_DISTANCE)
            && !self.stuck
            && self.close_to_home_try_ticks <= reduced_tick_delay(GIVE_UP_TICKS)
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
        let close_to_home = closer_to_center_than(home_pos, mob.position(), NEAR_HOME_DISTANCE);
        if close_to_home {
            self.close_to_home_try_ticks += 1;
        }

        if !mob.mob_base().navigation().lock().is_done() {
            return;
        }

        let home_vec = bottom_center(home_pos);
        let mut next =
            default_random_pos_towards(mob, TOWARD_TARGET_H, TOWARD_TARGET_V, home_vec, PI / 10.0)
                .or_else(|| {
                    default_random_pos_towards(
                        mob,
                        TOWARD_TARGET_FALLBACK_H,
                        TOWARD_TARGET_FALLBACK_V,
                        home_vec,
                        FRAC_PI_2,
                    )
                });

        if let Some(candidate) = next
            && !close_to_home
            && mob.level().is_some_and(|world| {
                world
                    .get_block_state(BlockPos::containing(candidate.x, candidate.y, candidate.z))
                    .get_block()
                    != &vanilla_blocks::WATER
            })
        {
            next = default_random_pos_towards(
                mob,
                TOWARD_TARGET_H,
                AVOID_WATER_V,
                home_vec,
                FRAC_PI_2,
            );
        }

        let Some(next) = next else {
            self.stuck = true;
            return;
        };
        mob.move_to_pos(next, self.speed_modifier);
    }
}

/// Vanilla `Turtle.TurtleRandomStrollGoal`: stroll only on land, and never while
/// heading home or carrying an egg.
pub(crate) struct TurtleRandomStrollGoal {
    inner: RandomStrollGoal,
}

impl TurtleRandomStrollGoal {
    pub(crate) const fn new(speed_modifier: f64, interval: i32) -> Self {
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
