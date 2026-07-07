//! aims the mobs look control at look target.

use super::{Behavior, BehaviorState};
use crate::entity::PathfinderMob;
use crate::entity::ai::brain::memory::{Memories, MemoryModuleType, MemoryStatus};
use crate::entity::ai::control::{DEFAULT_LOOK_X_MAX_ROT_ANGLE, DEFAULT_LOOK_Y_MAX_ROT_SPEED};

const ENTRTY_CONDITION: &[(MemoryModuleType, MemoryStatus)] =
    &[(MemoryModuleType::LookTarget, MemoryStatus::ValuePresent)];

pub(crate) struct LookAtTargetSink {
    state: BehaviorState,
}

impl LookAtTargetSink {
    #[must_use]
    pub(crate) const fn new(min_duration: i32, max_duration: i32) -> Self {
        Self {
            state: BehaviorState::with_min_max_duration(
                ENTRTY_CONDITION,
                min_duration,
                max_duration,
            ),
        }
    }
}

impl Behavior for LookAtTargetSink {
    fn state(&self) -> &BehaviorState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut BehaviorState {
        &mut self.state
    }

    fn can_still_use(&mut self, mob: &dyn PathfinderMob, memories: &Memories, _time: i64) -> bool {
        memories
            .look_target()
            .is_some_and(|target| target.is_visivle_by(mob))
    }

    fn tick(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories, _time: i64) {
        if let Some(target) = memories.look_target() {
            let position = target.current_position();
            mob.mob_base().controls().lock().look_control.set_look_at(
                position,
                DEFAULT_LOOK_Y_MAX_ROT_SPEED,
                DEFAULT_LOOK_X_MAX_ROT_ANGLE,
            );
        }
    }

    fn stop(&mut self, _mob: &dyn PathfinderMob, memories: &mut Memories, _time: i64) {
        memories.erase(MemoryModuleType::LookTarget);
    }
}
