use crate::advancement::criterion::{CriterionTrigger, CriterionTriggerInstance};
use crate::advancement::display::DisplayInfo;
use crate::loot_table::LootTableRef;
use crate::recipe::UntypedRecipeRef;
use std::cmp::PartialEq;
use std::collections::BTreeMap;
use std::fmt::Display;
use std::hash::{Hash, Hasher};
use steel_utils::Identifier;

pub mod registry;
pub mod display;
pub mod criterion;
pub mod tree;
pub mod positioner;

pub struct Advancement {
    pub key: Identifier,
    pub parent: Option<Identifier>,
    pub criteria: BTreeMap<String, dyn CriterionTrigger<dyn CriterionTriggerInstance>>,
    pub display: Option<DisplayInfo>,
    pub send_telemetry_event: bool,
    pub requirements: Vec<Vec<&'static str>>,
    pub rewards: AdvancementRewards,
}

impl Advancement {
    #[inline]
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.parent.is_none()
    }
}

impl PartialEq for &Advancement {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for &Advancement {}

impl Display for Advancement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.key)
    }
}

pub struct AdvancementRewards {
    pub experience: i32,
    pub loots: Vec<LootTableRef>,
    pub recipes: Vec<UntypedRecipeRef>,
    pub function: Option<Identifier>,
}