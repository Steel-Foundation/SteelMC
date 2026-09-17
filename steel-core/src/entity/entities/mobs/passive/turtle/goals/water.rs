use std::f64::consts::{FRAC_PI_2, PI};

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::vanilla_blocks;
use steel_utils::BlockPos;

use super::{
    TOWARD_TARGET_FALLBACK_H, TOWARD_TARGET_FALLBACK_V, TOWARD_TARGET_H, TOWARD_TARGET_V,
    as_turtle, bottom_center,
};
use crate::entity::ai::goal::{Goal, GoalControls, MoveToBlockGoal, default_random_pos_towards};
use crate::entity::{AgeableMob, Animal, PathfinderMob};
use crate::world::LevelReader;

const GO_TO_WATER_SEARCH_RANGE: i32 = 24;
const GO_TO_WATER_RECALC_INTERVAL: i32 = 160;
const TRAVEL_RANGE_XZ: i32 = 512;
const TRAVEL_RANGE_Y: i32 = 4;
const TRAVEL_LOADED_MARGIN: i32 = 34;

/// Leaves land for the nearest water block.
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

/// Picks a far swim target and wanders to it.
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

    #[cfg(test)]
    pub(crate) const fn is_stuck(&self) -> bool {
        self.stuck
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
        if yt + position.y > f64::from(LevelReader::sea_level(&world) - 1) {
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

        let Some(world) = mob.level() else {
            return;
        };
        let next_block = BlockPos::containing(next.x, next.y, next.z);
        if !world.are_full_chunks_loaded_at(
            next_block.offset(-TRAVEL_LOADED_MARGIN, 0, -TRAVEL_LOADED_MARGIN),
            next_block.offset(TRAVEL_LOADED_MARGIN, 0, TRAVEL_LOADED_MARGIN),
        ) {
            self.stuck = true;
            return;
        }

        mob.move_to_pos(next, self.speed_modifier);
    }
}
