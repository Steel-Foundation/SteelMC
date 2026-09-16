use crate::advancement::Advancement;
use crate::advancement::tree::AdvancementTree;
use rustc_hash::FxHashMap;
use std::sync::RwLock;
use steel_utils::Identifier;

pub static ADVANCEMENT_TREE: RwLock<AdvancementTree> = RwLock::new(AdvancementTree::default());
pub type AdvancementRef = &'static Advancement;

/// equivalent of the `AdvancementTree` of the minecraft source code
pub struct AdvancementRegistry {
    advancements: Vec<AdvancementRef>,
    by_key: FxHashMap<Identifier, usize>,
    roots: Vec<usize>,
    tasks: Vec<usize>,
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
            roots: Vec::new(),
            tasks: Vec::new(),
        }
    }

    pub(crate) fn register(&mut self, advancement: AdvancementRef) -> usize {
        let id = self.advancements.len();
        self.by_key.insert(advancement.key.clone(), id);
        self.advancements.push(advancement);
        id
    }

    pub(crate) fn register_multiple(&mut self, advancements: Vec<AdvancementRef>) {
        for advancement in &advancements {
            let is_root = advancement.is_root();
            let idx = self.register(*advancement);
            if is_root {
                self.roots.push(idx);
            }
        }
    }

    pub fn register_and_load(&mut self, advancements: Vec<AdvancementRef>) {
        self.register_multiple(advancements);
        self.reload()
    }

    pub fn iter(&self) -> impl Iterator<Item=(usize, AdvancementRef)> + '_ {
        self.advancements
            .iter()
            .enumerate()
            .map(|(id, &table)| (id, table))
    }

    pub fn reload(&mut self) {
        for advancement in self.roots {
            let advancement = self.advancements[advancement];
            positioner::run(self, &advancement);
        }
        self.allows_registering = false;
    }
}