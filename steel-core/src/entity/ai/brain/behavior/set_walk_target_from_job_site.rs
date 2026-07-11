//! walks the villager toward it's job site

use super::{Behavior, BehaviorState};
use crate::entity::PathfinderMob;
use crate::entity::ai::brain::memory::{Memories, MemoryModuleType, MemoryStatus, WalkTarget};

const ENTRY_CONDITION: &[(MemoryModuleType, MemoryStatus)] = &[
    (MemoryModuleType::JobSite, MemoryStatus::ValuePresent),
    (MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent),
];

pub(crate) struct SetWalkTargetFromJobSite {
    state: BehaviorState,
    speed_modifier: f32,
    close_enough_dist: i32,
}

impl SetWalkTargetFromJobSite {
    #[must_use]
    pub(crate) const fn new(speed_modifier: f32, close_enough_dist: i32) -> Self {
        Self {
            state: BehaviorState::new(ENTRY_CONDITION),
            speed_modifier,
            close_enough_dist,
        }
    }
}

impl Behavior for SetWalkTargetFromJobSite {
    fn state(&self) -> &BehaviorState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut BehaviorState {
        &mut self.state
    }

    fn start(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories, _time: i64) {
        let Some(job) = memories.job_site() else {
            return;
        };
        let origin = mob.block_position();
        let distance = (job.x() - origin.x()).abs()
            + (job.y() - origin.y()).abs()
            + (job.z() - origin.z()).abs();
        if distance > self.close_enough_dist {
            memories.set_walk_target(WalkTarget::from_block(
                job,
                self.speed_modifier,
                self.close_enough_dist,
            ));
        }
    }
}
