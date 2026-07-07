//! sets a random reachable walk target

use steel_utils::BlockPos;

use super::{Behavior, BehaviorState};
use crate::entity::PathfinderMob;
use crate::entity::ai::brain::memory::{Memories, MemoryModuleType, MemoryStatus, WalkTarget};
use crate::entity::ai::goal::default_random_pos;

const ENTRY_CONDITION: &[(MemoryModuleType, MemoryStatus)] =
    &[(MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent)];

const HORIZONTAL_RANGE: i32 = 10;
const VERTICAL_RANGE: i32 = 7;

pub(crate) struct RandomStroll {
    state: BehaviorState,
    speed_modifier: f32,
}

impl RandomStroll {
    #[must_use]
    pub(crate) const fn new(speed_modifier: f32) -> Self {
        Self {
            state: BehaviorState::new(ENTRY_CONDITION),
            speed_modifier,
        }
    }
}

impl Behavior for RandomStroll {
    fn state(&self) -> &BehaviorState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut BehaviorState {
        &mut self.state
    }

    fn start(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories, _time: i64) {
        if let Some(position) = default_random_pos(mob, HORIZONTAL_RANGE, VERTICAL_RANGE) {
            memories.set_walk_target(WalkTarget::from_block(
                BlockPos::from(position),
                self.speed_modifier,
                0,
            ));
        }
    }
}
