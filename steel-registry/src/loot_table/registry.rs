use super::{FxHashMap, Identifier, LootTable};

pub type LootTableRef = &'static LootTable;

/// Registry for loot tables.
pub struct LootTableRegistry {
    tables_by_id: Vec<LootTableRef>,
    tables_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl LootTableRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tables_by_id: Vec::new(),
            tables_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, table: LootTableRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register loot tables after the registry has been frozen"
        );

        let id = self.tables_by_id.len();
        self.tables_by_key.insert(table.key.clone(), id);
        self.tables_by_id.push(table);
        id
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, LootTableRef)> + '_ {
        self.tables_by_id
            .iter()
            .enumerate()
            .map(|(id, &table)| (id, table))
    }
}

impl Default for LootTableRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(
    LootTableRegistry,
    LootTable,
    tables_by_id,
    tables_by_key,
    loot_tables
);
