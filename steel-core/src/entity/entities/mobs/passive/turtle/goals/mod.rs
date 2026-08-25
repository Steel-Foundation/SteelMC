//! Bespoke turtle AI goals.
//!
//! These port the private goal classes nested inside vanilla 26.2 `Turtle`. They
//! read turtle-specific state (`has_egg`, `going_home`, `travel_pos`, the home
//! beach, and the lay-egg counter) that the shared goals cannot express, so they
//! live alongside the entity rather than in the generic goal module. The goals are
//! grouped by theme: [`breeding`] (breed and lay egg), [`water`] (panic, go to
//! water, travel), and [`land`] (go home, stroll).
//!
//! Two vanilla mechanisms are approximated because Steel has no equivalent yet,
//! and both are called out in the pull request for review:
//!
//! * TODO(amphibious-navigation): vanilla turtles swim with a custom
//!   `TurtleMoveControl` (water buoyancy and reduced land speed) and an
//!   `AmphibiousPathNavigation`. Steel exposes neither a per-entity move control
//!   nor an amphibious navigator, so the turtle uses the default control and
//!   navigation together with a `WATER` pathfinding malus of `0.0`. Water motion
//!   is therefore not pixel-perfect until the shared navigator lands.
//! * `TurtleTravelGoal` in vanilla rejects a swim target whose destination chunks
//!   are not loaded. Steel has no loaded-area query available to a goal, so that
//!   guard is omitted; an unreachable target simply leaves the navigation idle and
//!   the goal stops through `can_continue_to_use`.

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
