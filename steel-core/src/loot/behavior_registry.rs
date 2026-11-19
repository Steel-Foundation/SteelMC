use std::sync::LazyLock;

use rustc_hash::FxHashMap;
use steel_utils::Identifier;

use super::{
    LootAsyncOutput, LootContext, LootFuture, LootOutput, LootTableRef, OperationResult, vanilla,
};

/// Direct Rust implementation for tables that require no chunk I/O.
pub type LootTableFunction = fn(&mut LootContext<'_>, &mut LootOutput<'_>) -> OperationResult<()>;
/// Resumable Rust implementation, preserving its frame while chunk I/O is pending.
pub type LootTableAsyncFunction =
    for<'a> fn(&'a mut LootContext<'_>, &'a mut dyn LootAsyncOutput) -> LootFuture<'a>;

/// Compiled implementations, built before the registry is shared with evaluators.
pub struct LootTableBehaviorRegistry {
    pub(super) functions: FxHashMap<Identifier, LootTableFunction>,
    pub(super) async_functions: FxHashMap<Identifier, LootTableAsyncFunction>,
}

impl Default for LootTableBehaviorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl LootTableBehaviorRegistry {
    /// Creates a registry with direct and resumable vanilla implementations.
    #[must_use]
    pub fn new() -> Self {
        let mut registry = Self {
            functions: FxHashMap::default(),
            async_functions: FxHashMap::default(),
        };
        vanilla::register(&mut registry);
        registry
    }

    /// Registers only a direct implementation, removing any old resumable one.
    /// Use `register_pair` to support both execution paths.
    pub fn register(&mut self, table: LootTableRef, function: LootTableFunction) {
        self.async_functions.remove(&table.key);
        self.functions.insert(table.key.clone(), function);
    }

    /// Replaces both execution paths with a resumable implementation.
    pub fn register_async(&mut self, table: LootTableRef, function: LootTableAsyncFunction) {
        self.functions.remove(&table.key);
        self.async_functions.insert(table.key.clone(), function);
    }

    /// Registers equivalent direct and resumable implementations together.
    pub fn register_pair(
        &mut self,
        table: LootTableRef,
        direct: LootTableFunction,
        resumable: LootTableAsyncFunction,
    ) {
        self.functions.insert(table.key.clone(), direct);
        self.async_functions.insert(table.key.clone(), resumable);
    }
}

pub(super) static VANILLA_TABLES: LazyLock<LootTableBehaviorRegistry> =
    LazyLock::new(LootTableBehaviorRegistry::new);
