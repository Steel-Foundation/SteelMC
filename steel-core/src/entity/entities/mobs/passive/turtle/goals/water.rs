//! Turtle water goals: panicking toward water, heading to water, and traveling.

use std::f64::consts::{FRAC_PI_2, PI};

use glam::DVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::{vanilla_blocks, vanilla_damage_type_tags};
use steel_utils::BlockPos;

use super::{
    TOWARD_TARGET_FALLBACK_H, TOWARD_TARGET_FALLBACK_V, TOWARD_TARGET_H, TOWARD_TARGET_V,
    as_turtle, bottom_center,
};
use crate::entity::ai::goal::{
    Goal, GoalControls, MoveToBlockGoal, default_random_pos, default_random_pos_towards,
    look_for_water,
};
use crate::entity::{AgeableMob, Animal, PathfinderMob};

/// Vanilla `TurtlePanicGoal`: horizontal range searched for water to flee into.
const PANIC_WATER_SEARCH_RANGE: i32 = 7;
/// Vanilla `TurtlePanicGoal`: horizontal and vertical radius for the random
/// escape position used when no water is close.
const PANIC_ESCAPE_H: i32 = 5;
const PANIC_ESCAPE_V: i32 = 4;
/// Vanilla `TurtleGoToWaterGoal`: search range handed to `MoveToBlockGoal`.
const GO_TO_WATER_SEARCH_RANGE: i32 = 24;
/// Vanilla `TurtleGoToWaterGoal.shouldRecalculatePath`: recalc every N `tryTicks`.
const GO_TO_WATER_RECALC_INTERVAL: i32 = 160;
/// Vanilla `TurtleTravelGoal`: half-width of the box a far swim target is drawn
/// from, horizontally and vertically (`random.nextInt(1025) - 512`, `nextInt(9) - 4`).
const TRAVEL_RANGE_XZ: i32 = 512;
const TRAVEL_RANGE_Y: i32 = 4;

/// Vanilla `Turtle.TurtlePanicGoal`: always try to reach water when panicking,
/// not only while on fire, then fall back to a random escape position.
pub(crate) struct TurtlePanicGoal {
    wanted_position: Option<DVec3>,
    speed_modifier: f64,
    is_running: bool,
}

impl TurtlePanicGoal {
    pub(crate) const fn new(speed_modifier: f64) -> Self {
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

        if let Some(water) = look_for_water(mob, PANIC_WATER_SEARCH_RANGE) {
            self.wanted_position = Some(DVec3::new(
                f64::from(water.x()),
                f64::from(water.y()),
                f64::from(water.z()),
            ));
            return true;
        }

        let Some(position) = default_random_pos(mob, PANIC_ESCAPE_H, PANIC_ESCAPE_V) else {
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

/// Vanilla `Turtle.TurtleGoToWaterGoal`: leave land for the nearest water block.
pub(crate) struct TurtleGoToWaterGoal {
    inner: MoveToBlockGoal,
}

impl TurtleGoToWaterGoal {
    pub(crate) fn new(speed_modifier: f64) -> Self {
        Self {
            inner: MoveToBlockGoal::new(speed_modifier, GO_TO_WATER_SEARCH_RANGE, |level, pos| {
                level.get_block_state(pos).get_block() == &vanilla_blocks::WATER
            })
            .with_vertical_search_start(-1)
            .with_recalculate_path_interval(GO_TO_WATER_RECALC_INTERVAL),
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

/// Vanilla `Turtle.TurtleTravelGoal`: pick a far swim target and wander to it.
pub(crate) struct TurtleTravelGoal {
    speed_modifier: f64,
    stuck: bool,
}

impl TurtleTravelGoal {
    pub(crate) const fn new(speed_modifier: f64) -> Self {
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
        let xt = f64::from(rand::random_range(-TRAVEL_RANGE_XZ..=TRAVEL_RANGE_XZ));
        let mut yt = f64::from(rand::random_range(-TRAVEL_RANGE_Y..=TRAVEL_RANGE_Y));
        let zt = f64::from(rand::random_range(-TRAVEL_RANGE_XZ..=TRAVEL_RANGE_XZ));
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
        let next =
            default_random_pos_towards(mob, TOWARD_TARGET_H, TOWARD_TARGET_V, target, PI / 10.0)
                .or_else(|| {
                    default_random_pos_towards(
                        mob,
                        TOWARD_TARGET_FALLBACK_H,
                        TOWARD_TARGET_FALLBACK_V,
                        target,
                        FRAC_PI_2,
                    )
                });

        let Some(next) = next else {
            self.stuck = true;
            return;
        };
        mob.move_to_pos(next, self.speed_modifier);
    }
}
