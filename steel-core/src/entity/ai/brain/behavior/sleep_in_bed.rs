//! The villager sleeps in its claimed bed during the Rest activity, and wakes
//! when Rest ends (the brain stops the behavior at dawn). Based on vanilla
//! `SleepInBed`.

use steel_utils::BlockPos;

use super::{Behavior, BehaviorState};
use crate::entity::PathfinderMob;
use crate::entity::ai::brain::memory::{Memories, MemoryModuleType, MemoryStatus};

const ENTRY_CONDITION: &[(MemoryModuleType, MemoryStatus)] =
    &[(MemoryModuleType::Home, MemoryStatus::ValuePresent)];

/// Squared distance (blocks) the villager must be within to lie down.
const NEAR_BED_DIST_SQR: f64 = 4.0;
/// Sleep runs until Rest deactivates (the brain stops it at dawn); the duration
/// only needs to outlast a night so the behavior never self-times-out.
const SLEEP_DURATION: i32 = 24_000;

pub(crate) struct SleepInBed {
    state: BehaviorState,
}

impl SleepInBed {
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            state: BehaviorState::with_min_max_duration(
                ENTRY_CONDITION,
                SLEEP_DURATION,
                SLEEP_DURATION,
            ),
        }
    }
}

impl Behavior for SleepInBed {
    fn state(&self) -> &BehaviorState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut BehaviorState {
        &mut self.state
    }

    fn check_extra_start_conditions(
        &mut self,
        mob: &dyn PathfinderMob,
        memories: &Memories,
    ) -> bool {
        !mob.is_sleeping() && memories.home().is_some_and(|bed| near_bed(mob, bed))
    }

    fn start(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories, _time: i64) {
        if let Some(bed) = memories.home() {
            mob.start_sleeping(bed);
        }
    }

    fn can_still_use(&mut self, mob: &dyn PathfinderMob, _memories: &Memories, _time: i64) -> bool {
        mob.is_sleeping()
    }

    fn stop(&mut self, mob: &dyn PathfinderMob, _memories: &mut Memories, _time: i64) {
        mob.stop_sleeping();
    }
}

fn near_bed(mob: &dyn PathfinderMob, bed: BlockPos) -> bool {
    let pos = mob.position();
    let dx = (f64::from(bed.x()) + 0.5) - pos.x;
    let dy = (f64::from(bed.y()) + 0.5) - pos.y;
    let dz = (f64::from(bed.z()) + 0.5) - pos.z;
    dx * dx + dy * dy + dz * dz < NEAR_BED_DIST_SQR
}
