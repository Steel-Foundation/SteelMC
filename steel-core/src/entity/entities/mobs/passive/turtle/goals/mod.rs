//! Bespoke turtle AI goals, porting the private goal classes nested in vanilla
//! 26.2 `Turtle`. They read turtle state the shared goals cannot express, and are
//! grouped by theme: [`breeding`], [`water`], and [`land`].
//!
//! TODO(amphibious-navigation): vanilla turtles swim with an
//! `AmphibiousPathNavigation`. Steel has none, so the turtle uses the default
//! navigation with a `0.0` `WATER` malus until the shared navigator lands.

mod breeding;
mod land;
mod water;

use glam::DVec3;
use steel_utils::{BlockPos, Downcast as _};

use super::TurtleEntity;
use crate::entity::PathfinderMob;

pub(super) use breeding::{TurtleBreedGoal, TurtleLayEggGoal};
pub(super) use land::{TurtleGoHomeGoal, TurtleRandomStrollGoal};
pub(super) use water::{TurtleGoToWaterGoal, TurtlePanicGoal, TurtleTravelGoal};

// Vanilla `DefaultRandomPos.getPosTowards` radii for the go-home and travel
// goals: a primary attempt, then a wider fallback.
pub(super) const TOWARD_TARGET_H: i32 = 16;
pub(super) const TOWARD_TARGET_V: i32 = 3;
pub(super) const TOWARD_TARGET_FALLBACK_H: i32 = 8;
pub(super) const TOWARD_TARGET_FALLBACK_V: i32 = 7;

fn as_turtle(mob: &dyn PathfinderMob) -> Option<&TurtleEntity> {
    mob.downcast_ref::<TurtleEntity>()
}

/// Vanilla `BlockPos.closerToCenterThan`: squared distance from the block center.
pub(super) fn closer_to_center_than(block: BlockPos, position: DVec3, distance: f64) -> bool {
    let (x, y, z) = block.get_center();
    DVec3::new(x, y, z).distance_squared(position) < distance * distance
}

/// Vanilla `Vec3.atBottomCenterOf`.
fn bottom_center(block: BlockPos) -> DVec3 {
    let (x, y, z) = block.get_bottom_center();
    DVec3::new(x, y, z)
}
