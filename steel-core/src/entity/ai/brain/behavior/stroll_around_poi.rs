//! Strolls to random nearby positions around the job-site POI while the
//! villager is already close to it. Based on vanilla `StrollAroundPoi`.

use steel_utils::BlockPos;

use super::{Behavior, BehaviorState};
use crate::entity::PathfinderMob;
use crate::entity::ai::brain::memory::{Memories, MemoryModuleType, MemoryStatus, WalkTarget};
use crate::entity::ai::goal::default_random_pos;

const ENTRY_CONDITION: &[(MemoryModuleType, MemoryStatus)] = &[
    (MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent),
    (MemoryModuleType::JobSite, MemoryStatus::ValuePresent),
];

const HORIZONTAL_RANGE: i32 = 8;
const VERTICAL_RANGE: i32 = 6;
const MIN_TIME_BETWEEN_STROLLS: i64 = 180;

pub(crate) struct StrollAroundPoi {
    state: BehaviorState,
    speed_modifier: f32,
    max_distance: i32,
    next_ok_start: i64,
}

impl StrollAroundPoi {
    #[must_use]
    pub(crate) const fn new(speed_modifier: f32, max_distance: i32) -> Self {
        Self {
            state: BehaviorState::new(ENTRY_CONDITION),
            speed_modifier,
            max_distance,
            next_ok_start: 0,
        }
    }
}

impl Behavior for StrollAroundPoi {
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
        let Some(world) = mob.level() else {
            return false;
        };
        if world.game_time() <= self.next_ok_start {
            return false;
        }
        memories
            .job_site()
            .is_some_and(|job| within_distance(mob, job, self.max_distance))
    }

    fn start(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories, time: i64) {
        if let Some(position) = default_random_pos(mob, HORIZONTAL_RANGE, VERTICAL_RANGE) {
            memories.set_walk_target(WalkTarget::from_block(
                BlockPos::from(position),
                self.speed_modifier,
                1,
            ));
        }
        self.next_ok_start = time + MIN_TIME_BETWEEN_STROLLS;
    }
}

fn within_distance(mob: &dyn PathfinderMob, poi: BlockPos, max_distance: i32) -> bool {
    let pos = mob.position();
    let dx = (f64::from(poi.x()) + 0.5) - pos.x;
    let dy = (f64::from(poi.y()) + 0.5) - pos.y;
    let dz = (f64::from(poi.z()) + 0.5) - pos.z;
    let max = f64::from(max_distance);
    dx * dx + dy * dy + dz * dz < max * max
}
