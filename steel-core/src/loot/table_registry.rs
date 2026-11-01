use std::sync::LazyLock;

use rustc_hash::FxHashMap;
use steel_registry::RegistryExt;
use steel_utils::Identifier;

use super::{LootTable, LootTableRef, vanilla_loot_tables};

/// Registered vanilla loot-table identities, independent of their execution context.
pub static LOOT_TABLES: LazyLock<LootTableRegistry> = LazyLock::new(|| {
    let mut registry = LootTableRegistry::new();
    vanilla_loot_tables::register_loot_tables(&mut registry);
    registry.freeze();
    registry
});

/// Registry for loot tables.
pub struct LootTableRegistry {
    tables_by_id: Vec<LootTableRef>,
    tables_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl LootTableRegistry {
    /// Creates an empty registry accepting table registrations.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tables_by_id: Vec::new(),
            tables_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    /// Registers a table identity and returns its registry index.
    ///
    /// # Panics
    /// Panics if the registry has been frozen.
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

    /// Iterates table identities in registration order.
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

steel_registry::impl_registry_ext!(LootTableRegistry, LootTable, tables_by_id, tables_by_key);
