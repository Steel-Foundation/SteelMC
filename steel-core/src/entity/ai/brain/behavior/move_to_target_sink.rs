//! walks the mob towards it's walk target.

use super::{Behavior, BehaviorState};
use crate::entity::PathfinderMob;
use crate::entity::ai::brain::memory::{Memories, MemoryModuleType, MemoryStatus};

const ENTRY_CONDITION: &[(MemoryModuleType, MemoryStatus)] =
    &[(MemoryModuleType::WalkTarget, MemoryStatus::ValuePresent)];

pub(crate) struct MoveToTargetSink {
    state: BehaviorState,
}

impl MoveToTargetSink {
    #[must_use]
    pub(crate) const fn new(min_duration: i32, max_duration: i32) -> Self {
        Self {
            state: BehaviorState::with_min_max_duration(
                ENTRY_CONDITION,
                min_duration,
                max_duration,
            ),
        }
    }
}

impl Behavior for MoveToTargetSink {
    fn state(&self) -> &BehaviorState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut BehaviorState {
        &mut self.state
    }

    fn start(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories, _time: i64) {
        let Some(walk_target) = memories.walk_target() else {
            return;
        };
        let position = walk_target.target().current_position();
        let speed_modifier = f64::from(walk_target.speed_modifier());
        mob.move_to_pos(position, speed_modifier);
    }

    fn can_still_use(&mut self, mob: &dyn PathfinderMob, memories: &Memories, _time: i64) -> bool {
        memories.has_value(MemoryModuleType::WalkTarget)
            && !mob.has_controlling_passenger()
            && !mob.mob_base().navigation().lock().is_done()
    }

    fn stop(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories, _time: i64) {
        mob.mob_base().navigation().lock().stop();
        memories.erase(MemoryModuleType::WalkTarget);
    }
}
