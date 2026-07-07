//! Villager capability trait

use steel_registry::entity_data::VillagerData;

use crate::entity::Mob;

pub trait Villager: Mob {
    fn villager_data(&self) -> VillagerData;
    fn set_villager_data(&self, data: VillagerData);
}
