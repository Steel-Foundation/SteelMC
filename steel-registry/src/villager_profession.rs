use rustc_hash::FxHashMap;
use steel_utils::Identifier;

use crate::poi::PoiTypeRef;
use crate::sound_event::SoundEventRef;

#[derive(Debug)]
pub struct VillagerProfession {
    pub key: Identifier,
    pub work_sound: Option<SoundEventRef>,
    /// Vanilla `VillagerProfession.heldJobSite` — the POI types a villager with this
    /// profession is employed at. Empty for `minecraft:none` and `minecraft:nitwit`.
    pub held_job_site: &'static [PoiTypeRef],
    /// Vanilla `VillagerProfession.acquirableJobSite` — the POI types that can grant
    /// this profession. `minecraft:none` acquires every `#minecraft:acquirable_job_site`
    /// member; other professions only re-acquire their own job site.
    pub acquirable_job_site: &'static [PoiTypeRef],
}

pub type VillagerProfessionRef = &'static VillagerProfession;

pub struct VillagerProfessionRegistry {
    villager_professions_by_id: Vec<VillagerProfessionRef>,
    villager_professions_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl VillagerProfessionRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            villager_professions_by_id: Vec::new(),
            villager_professions_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    VillagerProfessionRegistry,
    VillagerProfessionRef,
    villager_professions_by_id,
    villager_professions_by_key,
    allows_registering
);

crate::impl_registry!(
    VillagerProfessionRegistry,
    VillagerProfession,
    villager_professions_by_id,
    villager_professions_by_key,
    villager_professions
);
