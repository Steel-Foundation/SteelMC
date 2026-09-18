use crate::advancement::criterion::AnyCriterion;
use crate::advancement::display::DisplayInfo;
use crate::loot_table::LootTableRef;
use crate::recipe::UntypedRecipeRef;
use std::cmp::PartialEq;
use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};
use steel_utils::Identifier;

pub mod criterion;
pub mod display;
pub mod positioner;
pub mod registry;

pub struct Advancement {
    pub key: Identifier,
    pub parent: Option<Identifier>,
    pub criteria: BTreeMap<String, Box<dyn AnyCriterion>>,
    pub display: Option<DisplayInfo>,
    pub send_telemetry_event: bool,
    pub requirements: Vec<Vec<&'static str>>,
    pub rewards: AdvancementRewards,
}

impl Advancement {
    #[inline]
    #[must_use]
    pub const fn is_root(&self) -> bool {
        self.parent.is_none()
    }
}

impl PartialEq for Advancement {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for Advancement {}

impl Display for Advancement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.key)
    }
}

impl Debug for Advancement {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.key)
    }
}

pub struct AdvancementRewards {
    pub experience: i32,
    pub loots: Vec<LootTableRef>,
    pub recipes: Vec<UntypedRecipeRef>,
    pub function: Option<Identifier>,
}
