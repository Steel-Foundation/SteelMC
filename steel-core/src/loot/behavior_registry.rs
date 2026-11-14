use std::sync::LazyLock;

use rustc_hash::FxHashMap;
use steel_utils::Identifier;

use super::{LootContext, LootOutput, LootResult, LootTableRef, vanilla};

/// A compiled table function, also implementable by handwritten Rust code.
pub type LootTableFunction = fn(&mut LootContext<'_>, &mut LootOutput<'_>) -> LootResult<()>;

/// Maps table identities to compiled implementations. Build before sharing with contexts.
pub struct LootTableBehaviorRegistry {
    pub(super) functions: FxHashMap<Identifier, LootTableFunction>,
}

impl Default for LootTableBehaviorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl LootTableBehaviorRegistry {
    /// Creates a registry with the generated vanilla implementations.
    #[must_use]
    pub fn new() -> Self {
        let mut registry = Self {
            functions: FxHashMap::default(),
        };
        vanilla::register(&mut registry);
        registry
    }

    /// Registers or replaces the Rust implementation of a table identity.
    pub fn register(&mut self, table: LootTableRef, function: LootTableFunction) {
        self.functions.insert(table.key.clone(), function);
    }
}

pub(super) static VANILLA_TABLES: LazyLock<LootTableBehaviorRegistry> =
    LazyLock::new(LootTableBehaviorRegistry::new);
