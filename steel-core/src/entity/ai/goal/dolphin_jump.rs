//! Vanilla `DolphinJumpGoal` — periodically jumps out of the water.
//!
//! Dolphins breach the surface at a configurable interval, giving them their
//! characteristic leaping behavior.  The goal only activates when the dolphin
//! is submerged and has enough water below to push off from.

use super::selector::{Goal, GoalControls};
use crate::entity::PathfinderMob;
use crate::fluid::FluidStateExt as _;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;

/// Default vanilla jump interval (no randomised cooldown — fires every tick
/// the interval check passes).
const DEFAULT_JUMP_INTERVAL: i32 = 2;

pub struct DolphinJumpGoal {
    interval: i32,
}

impl DolphinJumpGoal {
    #[must_use]
    pub(crate) const fn new(interval: i32) -> Self {
        Self { interval }
    }
}

impl Goal for DolphinJumpGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::JUMP
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        if mob.tick_count() % self.interval != 0 {
            return false;
        }
        // Must be in water and not already airborne.
        if !mob.is_in_water() {
            return false;
        }
        // Only jump when the water surface is close above — the dolphin
        // should be near the top layer, not deep underwater.
        let Some(world) = mob.level() else {
            return false;
        };
        let pos = mob.block_position();
        let above = pos.above();
        let state_above = world.get_block_state(above);
        // The block above should be air or non-water (i.e. the surface is here).
        !state_above.get_fluid_state().is_water()
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        mob.jump_control_jump();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dolphin_jump_goal_claims_jump_control() {
        let goal = DolphinJumpGoal::new(2);
        assert_eq!(goal.controls(), GoalControls::JUMP);
    }

    #[test]
    fn dolphin_jump_goal_requires_update_every_tick() {
        let goal = DolphinJumpGoal::new(2);
        assert!(goal.requires_update_every_tick());
    }
}
