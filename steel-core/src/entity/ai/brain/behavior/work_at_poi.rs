use super::{Behavior, BehaviorState};
use crate::entity::{PathfinderMob, Villager};
use crate::entity::ai::brain::memory::{Memories, MemoryModuleType, MemoryStatus, PositionTracker};

const ENTRY_CONDITION: &[(MemoryModuleType, MemoryStatus)] =
    &[(MemoryModuleType::JobSite, MemoryStatus::ValuePresent)];

const CHECK_COOLDOWN: i64 = 300;
const DISTANCE_SQR: f64 = 1.73 * 1.73;

pub(crate) struct WorkAtPoi {
    state: BehaviorState,
    last_check: i64,
}

impl WorkAtPoi {
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self { state: BehaviorState::new(ENTRY_CONDITION), last_check: 0 }
    }
}

impl Behavior for WorkAtPoi {
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
    ) -> bool
    {
        let Some(world) = mob.level() else { return false };
        let now = world.game_time();
        if now - self.last_check < CHECK_COOLDOWN {
            return false;
        }
        self.last_check = now;
        let Some(job) = memories.job_site() else { return false };
        let pos = mob.position();
        let dx = (job.x() as f64 + 0.5) - pos.x;
        let dy = (job.y() as f64 + 0.5) - pos.y;
        let dz = (job.z() as f64 + 0.5) - pos.z;
        dx * dx + dy * dy + dz * dz < DISTANCE_SQR
    }

    fn start(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories, _time: i64) {
        if let Some(job) = memories.job_site() {
            memories.set_look_target(PositionTracker::Block(job));
        }
        // TODO play professions work sound
        if let Some(villager) = mob.as_villager() {
            villager.try_restock();
        }
    }
}
