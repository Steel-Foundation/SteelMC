use crate::advancement::Advancement;
use rustc_hash::FxHashMap;
use steel_utils::Identifier;

pub type AdvancementRef = &'static Advancement;

pub struct AdvancementRegistry {
    advancements: Vec<AdvancementRef>,
    by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl AdvancementRegistry {
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