//! Brain behaviors

use super::memory::{Memories, MemoryModuleType, MemoryStatus};
use crate::entity::PathfinderMob;

mod acquire_bed;
mod acquire_job_site;
mod assign_profession;
mod look_at_target_sink;
mod move_to_target_sink;
mod random_stroll;
mod set_entity_look_target;
mod set_walk_target_from_home;
mod set_walk_target_from_job_site;
mod work_at_poi;

pub(crate) use acquire_bed::AcquireBed;
pub(crate) use acquire_job_site::AcquireJobSite;
pub(crate) use assign_profession::AssignProfession;
pub(crate) use look_at_target_sink::LookAtTargetSink;
pub(crate) use move_to_target_sink::MoveToTargetSink;
pub(crate) use random_stroll::RandomStroll;
pub(crate) use set_entity_look_target::SetEntityLookTarget;
pub(crate) use set_walk_target_from_home::SetWalkTargetFromHome;
pub(crate) use work_at_poi::WorkAtPoi;
pub(crate) use set_walk_target_from_job_site::SetWalkTargetFromJobSite;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BehaviorStatus {
    Stopped,
    Running,
}

pub(crate) struct BehaviorState {
    entry_condition: &'static [(MemoryModuleType, MemoryStatus)],
    status: BehaviorStatus,
    min_duration: i32,
    max_duration: i32,
    end_timestamp: i64,
}

impl BehaviorState {
    const DEFAULT_DURATION: i32 = 60;

    #[must_use]
    pub(crate) const fn new(entry_condition: &'static [(MemoryModuleType, MemoryStatus)]) -> Self {
        Self::with_duration(entry_condition, Self::DEFAULT_DURATION)
    }

    #[must_use]
    pub(crate) const fn with_duration(
        entity_condition: &'static [(MemoryModuleType, MemoryStatus)],
        duration: i32,
    ) -> Self {
        Self::with_min_max_duration(entity_condition, duration, duration)
    }

    #[must_use]
    pub(crate) const fn with_min_max_duration(
        entry_condition: &'static [(MemoryModuleType, MemoryStatus)],
        min_duration: i32,
        max_duration: i32,
    ) -> Self {
        Self {
            entry_condition,
            status: BehaviorStatus::Stopped,
            min_duration,
            max_duration,
            end_timestamp: 0,
        }
    }

    fn roll_duration(&self) -> i32 {
        self.min_duration + rand::random_range(0..(self.max_duration - self.min_duration + 1))
    }

    const fn timed_out(&self, time: i64) -> bool {
        time > self.end_timestamp
    }
}

pub(crate) trait Behavior: Send {
    fn state(&self) -> &BehaviorState;

    fn state_mut(&mut self) -> &mut BehaviorState;

    fn check_extra_start_conditions(
        &mut self,
        _mob: &dyn PathfinderMob,
        _memories: &Memories,
    ) -> bool {
        true
    }

    fn start(&mut self, _mob: &dyn PathfinderMob, _memories: &mut Memories, _time: i64) {}

    fn tick(&mut self, _mob: &dyn PathfinderMob, _memories: &mut Memories, _time: i64) {}

    fn can_still_use(
        &mut self,
        _mob: &dyn PathfinderMob,
        _memories: &Memories,
        _time: i64,
    ) -> bool {
        false
    }

    fn stop(&mut self, _mob: &dyn PathfinderMob, _memories: &mut Memories, _time: i64) {}

    fn status(&self) -> BehaviorStatus {
        self.state().status
    }

    fn try_start(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories, time: i64) -> bool {
        if self.has_required_memories(memories) && self.check_extra_start_conditions(mob, memories)
        {
            {
                let state = self.state_mut();
                state.status = BehaviorStatus::Running;
                let duration = state.roll_duration();
                state.end_timestamp = time + i64::from(duration);
            }
            self.start(mob, memories, time);
            true
        } else {
            false
        }
    }

    fn tick_or_stop(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories, time: i64) {
        if !self.state().timed_out(time) && self.can_still_use(mob, memories, time) {
            self.tick(mob, memories, time);
        } else {
            self.do_stop(mob, memories, time);
        }
    }

    fn do_stop(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories, time: i64) {
        self.state_mut().status = BehaviorStatus::Stopped;
        self.stop(mob, memories, time);
    }

    fn has_required_memories(&self, memories: &Memories) -> bool {
        self.state()
            .entry_condition
            .iter()
            .all(|(ty, status)| memories.check_memory(*ty, *status))
    }
}
