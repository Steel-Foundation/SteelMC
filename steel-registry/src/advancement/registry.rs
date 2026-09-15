use crate::advancement::{Advancement, AdvancementTree};
use rustc_hash::FxHashMap;
use std::sync::RwLock;
use steel_utils::Identifier;

pub static ADVANCEMENT_TREE: RwLock<AdvancementTree> = RwLock::new(AdvancementTree::default());
pub type AdvancementRef = &'static Advancement;

pub struct AdvancementRegistry {
    advancements: Vec<AdvancementRef>,
    by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl Default for AdvancementRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AdvancementRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            advancements: Vec::new(),
            by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, advancement: AdvancementRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register loot tables after the registry has been frozen"
        );

        let id = self.advancements.len();
        self.by_key.insert(advancement.key.clone(), id);
        self.advancements.push(advancement);
        id
    }

    pub fn iter(&self) -> impl Iterator<Item=(usize, AdvancementRef)> + '_ {
        self.advancements
            .iter()
            .enumerate()
            .map(|(id, &table)| (id, table))
    }
}