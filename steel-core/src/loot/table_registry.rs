use std::sync::LazyLock;

use rustc_hash::FxHashMap;
use steel_registry::RegistryExt;
use steel_utils::Identifier;

use super::{LootTable, LootTableRef, vanilla_loot_tables};

/// Registered vanilla loot-table identities, independent of their execution context.
// TODO: Allow loot table customization before freezing, through the same
// initialization flow as the other registries.
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
}

steel_registry::impl_standard_methods!(
    LootTableRegistry,
    LootTableRef,
    tables_by_id,
    tables_by_key,
    allows_registering,
    "Cannot register duplicate loot table key: {}"
);

steel_registry::impl_registry_ext!(LootTableRegistry, LootTable, tables_by_id, tables_by_key);

#[cfg(test)]
mod tests {
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        ptr,
    };

    use super::*;

    #[test]
    fn duplicate_key_rejection_preserves_registered_table() {
        static ORIGINAL: LootTable = LootTable {
            key: Identifier::new_static("steel", "test/duplicate"),
            param_set: Identifier::vanilla_static("generic"),
            random_sequence: None,
        };
        static DUPLICATE: LootTable = LootTable {
            key: Identifier::new_static("steel", "test/duplicate"),
            param_set: Identifier::vanilla_static("block"),
            random_sequence: None,
        };

        let mut registry = LootTableRegistry::new();
        let id = registry.register(&ORIGINAL);
        let result = catch_unwind(AssertUnwindSafe(|| registry.register(&DUPLICATE)));

        assert!(result.is_err());
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.id_from_key(&ORIGINAL.key), Some(id));
        assert!(
            registry
                .by_id(id)
                .is_some_and(|table| ptr::eq(table, &raw const ORIGINAL))
        );
        assert!(
            registry
                .by_key(&ORIGINAL.key)
                .is_some_and(|table| ptr::eq(table, &raw const ORIGINAL))
        );
    }
}
