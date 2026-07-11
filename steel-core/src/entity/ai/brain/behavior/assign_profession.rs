//! gives a jobless villager the profession of its jobsite.

use steel_registry::{REGISTRY, RegistryExt, vanilla_villager_professions};

use super::{Behavior, BehaviorState};
use crate::entity::PathfinderMob;
use crate::entity::ai::brain::memory::{Memories, MemoryModuleType, MemoryStatus};

const ENTRY_CONDITION: &[(MemoryModuleType, MemoryStatus)] =
    &[(MemoryModuleType::JobSite, MemoryStatus::ValuePresent)];

pub(crate) struct AssignProfession {
    state: BehaviorState,
}

impl AssignProfession {
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            state: BehaviorState::new(ENTRY_CONDITION),
        }
    }
}

impl Default for AssignProfession {
    fn default() -> Self {
        Self::new()
    }
}

impl Behavior for AssignProfession {
    fn state(&self) -> &BehaviorState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut BehaviorState {
        &mut self.state
    }

    fn check_extra_start_conditions(
        &mut self,
        mob: &dyn PathfinderMob,
        _memories: &Memories,
    ) -> bool {
        let Some(villager) = mob.as_villager() else {
            return false;
        };
        let Some(none_id) = REGISTRY
            .villager_professions
            .id_from_key(&vanilla_villager_professions::NONE.key)
        else {
            return false;
        };
        let Ok(none_id) = i32::try_from(none_id) else {
            return false;
        };
        villager.villager_data().profession == none_id
    }

    fn start(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories, _time: i64) {
        let Some(villager) = mob.as_villager() else {
            return;
        };
        let Some(world) = mob.level() else {
            return;
        };
        let Some(job_site) = memories.job_site() else {
            return;
        };
        let Some(poi_type_id) = world.poi_storage.lock().get_type(job_site) else {
            return;
        };
        let Some(poi_type) = REGISTRY.poi_types.by_id(poi_type_id) else {
            return;
        };
        let Some(profession_id) = REGISTRY.villager_professions.id_from_key(&poi_type.key) else {
            return;
        };
        let Ok(profession_id) = i32::try_from(profession_id) else {
            return;
        };

        let mut data = villager.villager_data();
        data.profession = profession_id;
        villager.set_villager_data(data);
        villager.update_trades();
    }
}
