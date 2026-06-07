use rustc_hash::FxHashMap;
use steel_utils::Identifier;

use crate::sound_event::SoundEventRef;

#[derive(Debug)]
pub struct VillagerProfession {
    pub key: Identifier,
    pub work_sound: Option<SoundEventRef>,
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
