//! claims the nearsest job-site POI

use steel_registry::vanilla_poi_type_tags::PoiTag;
use steel_registry::{REGISTRY, RegistryExt, TaggedRegistryExt};

use super::{Behavior, BehaviorState};
use crate::entity::PathfinderMob;
use crate::entity::ai::brain::memory::{Memories, MemoryModuleType, MemoryStatus};
use crate::poi::OccupationStatus;

const ENTRY_CONDITION: &[(MemoryModuleType, MemoryStatus)] =
    &[(MemoryModuleType::JobSite, MemoryStatus::ValueAbsent)];

pub(crate) struct AcquireJobSite {
    state: BehaviorState,
    search_radius: i32,
}

impl AcquireJobSite {
    #[must_use]
    pub(crate) const fn new(search_radius: i32) -> Self {
        Self {
            state: BehaviorState::new(ENTRY_CONDITION),
            search_radius,
        }
    }
}

impl Behavior for AcquireJobSite {
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
        let is_job_site = |id: usize| {
            REGISTRY.poi_types.by_id(id).is_some_and(|ty| {
                REGISTRY
                    .poi_types
                    .is_in_tag(ty, &PoiTag::ACQUIRABLE_JOB_SITE)
            })
        };

        let origin = mob.block_position();
        let mut poi = world.poi_storage.lock();
        let Some((job_pos, _)) = poi.get_nearest(
            &is_job_site,
            origin,
            self.search_radius,
            OccupationStatus::Free,
        ) else {
            return;
        };
        if poi.reserve_ticket(job_pos) {
            memories.set_job_site(job_pos);
        }
    }
}
