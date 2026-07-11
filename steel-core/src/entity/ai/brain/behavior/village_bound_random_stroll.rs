//! Random stroll that heads back toward the villager's home/job-site POI when
//! it has strayed too far, and wanders freely otherwise. A POI-anchored
//! approximation of vanilla's `VillageBoundRandomStroll` (which uses the full
//! village-distance subsystem steel doesn't have yet).

use std::f64::consts::FRAC_PI_2;

use glam::DVec3;
use steel_utils::BlockPos;

use super::{Behavior, BehaviorState};
use crate::entity::PathfinderMob;
use crate::entity::ai::brain::memory::{Memories, MemoryModuleType, MemoryStatus, WalkTarget};
use crate::entity::ai::goal::{default_random_pos, default_random_pos_towards};

const ENTRY_CONDITION: &[(MemoryModuleType, MemoryStatus)] =
    &[(MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent)];

const HORIZONTAL_RANGE: i32 = 10;
const VERTICAL_RANGE: i32 = 7;
/// Beyond this horizontal distance from its anchor POI, the villager strolls
/// back toward it rather than wandering freely.
const VILLAGE_RADIUS: f64 = 32.0;

pub(crate) struct VillageBoundRandomStroll {
    state: BehaviorState,
    speed_modifier: f32,
}

impl VillageBoundRandomStroll {
    #[must_use]
    pub(crate) const fn new(speed_modifier: f32) -> Self {
        Self {
            state: BehaviorState::new(ENTRY_CONDITION),
            speed_modifier,
        }
    }
}

impl Behavior for VillageBoundRandomStroll {
    fn state(&self) -> &BehaviorState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut BehaviorState {
        &mut self.state
    }

    fn start(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories, _time: i64) {
        // Anchor to the bed if the villager has one, else its job site.
        let anchor = memories.home().or_else(|| memories.job_site());
        let position = match anchor {
            Some(anchor) if strayed_from(mob, anchor) => default_random_pos_towards(
                mob,
                HORIZONTAL_RANGE,
                VERTICAL_RANGE,
                anchor_center(anchor),
                FRAC_PI_2,
            ),
            _ => default_random_pos(mob, HORIZONTAL_RANGE, VERTICAL_RANGE),
        };

        if let Some(position) = position {
            memories.set_walk_target(WalkTarget::from_block(
                BlockPos::from(position),
                self.speed_modifier,
                0,
            ));
        }
    }
}

/// Returns true if the mob is farther than [`VILLAGE_RADIUS`] (horizontally)
/// from its anchor POI.
fn strayed_from(mob: &dyn PathfinderMob, anchor: BlockPos) -> bool {
    let pos = mob.position();
    let dx = (f64::from(anchor.x()) + 0.5) - pos.x;
    let dz = (f64::from(anchor.z()) + 0.5) - pos.z;
    dx * dx + dz * dz > VILLAGE_RADIUS * VILLAGE_RADIUS
}

fn anchor_center(anchor: BlockPos) -> DVec3 {
    DVec3::new(
        f64::from(anchor.x()) + 0.5,
        f64::from(anchor.y()),
        f64::from(anchor.z()) + 0.5,
    )
}
