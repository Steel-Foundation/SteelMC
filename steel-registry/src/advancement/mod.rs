use crate::advancement::display::DisplayInfo;
use crate::loot_table::LootTableRef;
use crate::recipe::UntypedRecipeRef;
use std::collections::BTreeMap;
use steel_utils::Identifier;

pub mod registry;
pub mod display;

pub struct Advancement {
    pub key: Identifier,
    pub parent: Option<Identifier>,
    pub criteria: BTreeMap<String, Criterion>,
    pub display: Option<DisplayInfo>,
    pub send_telemetry_event: bool,
    pub requirements: Vec<Vec<&'static str>>,
    pub rewards: AdvancementRewards,
}

pub struct AdvancementRewards {
    pub experience: i32,
    pub loots: Vec<LootTableRef>,
    pub recipes: Vec<UntypedRecipeRef>,
    pub function: Option<Identifier>,
}

pub struct Criterion {}

impl Criterion {
    pub const fn new() -> Self {
        Self {}
    }
}