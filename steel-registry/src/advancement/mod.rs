use crate::advancement::criterion::{CriterionTrigger, CriterionTriggerInstance};
use crate::advancement::display::DisplayInfo;
use crate::advancement::registry::ADVANCEMENT_TREE;
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

pub struct Advancement {
    pub key: Identifier,
    pub parent: Option<Identifier>,
    pub criteria: BTreeMap<String, dyn CriterionTrigger<dyn CriterionTriggerInstance>>,
    pub display: Option<DisplayInfo>,
    pub send_telemetry_event: bool,
    pub requirements: Vec<Vec<&'static str>>,
    pub rewards: AdvancementRewards,
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

pub struct AdvancementNode {
    pub children: Vec<usize>,
    pub parent: Option<usize>,
    pub value: &'static Advancement,
}


impl AdvancementNode {
    pub fn add_child(&mut self, child: usize) {
        self.children.push(child);
    }

    #[must_use]
    pub fn new(value: &'static Advancement, parent: Option<usize>) -> Self {
        Self {
            value,
            parent,
            children: Vec::new(),
        }
    }

    #[inline]
    #[must_use]
    pub const fn has_display(&self) -> bool {
        self.value.display.is_some()
    }

    pub fn root(&self) -> &AdvancementNode {
        let mut advancement_node = self;
        while let Some(parent) = &advancement_node.parent {
            advancement_node = &ADVANCEMENT_TREE.read().unwrap().nodes_vector[*parent];
        }
        advancement_node
    }
}

impl PartialEq<Self> for AdvancementNode {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for AdvancementNode {}

impl Display for AdvancementNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value.key)
    }
}
impl Hash for AdvancementNode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

#[derive(Default)]
pub struct AdvancementTree {
    pub nodes: BTreeMap<Identifier, usize>,
    pub nodes_vector: Vec<AdvancementNode>,
    pub roots: Vec<usize>,
    pub tasks: Vec<usize>,
}