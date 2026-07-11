//! claims the nearest free bed as the villager's home

use steel_registry::{REGISTRY, RegistryExt, vanilla_poi_types};

use super::{Behavior, BehaviorState};
use crate::entity::PathfinderMob;
use crate::entity::ai::brain::memory::{Memories, MemoryModuleType, MemoryStatus};
use crate::poi::OccupationStatus;

const ENTRY_CONDITION: &[(MemoryModuleType, MemoryStatus)] =
    &[(MemoryModuleType::Home, MemoryStatus::ValueAbsent)];

pub(crate) struct AcquireBed {
    state: BehaviorState,
    search_radius: i32,
}

impl AcquireBed {
    #[must_use]
    pub(crate) const fn new(search_radius: i32) -> Self {
        Self {
            state: BehaviorState::new(ENTRY_CONDITION),
            search_radius,
        }
    }
}

impl Behavior for AcquireBed {
    fn state(&self) -> &BehaviorState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut BehaviorState {
        &mut self.state
    }

    fn start(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories, _time: i64) {
        let Some(world) = mob.level() else {
            return;
        };
        let Some(home_id) = REGISTRY.poi_types.id_from_key(&vanilla_poi_types::HOME.key) else {
            return;
        };

        let origin = mob.block_position();
        let mut poi = world.poi_storage.lock();
        let Some((bed_pos, _)) = poi.get_nearest(
            &|id| id == home_id,
            origin,
            self.search_radius,
            OccupationStatus::Free,
        ) else {
            return;
        };
        if poi.reserve_ticket(bed_pos) {
            memories.set_home(bed_pos);
        }
    }
}
