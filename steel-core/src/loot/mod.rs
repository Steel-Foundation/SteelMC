//! Loot tables compiled from vanilla data into Rust functions.
//!
//! See [`LootTable`] for generation and [`LootTableBehaviorRegistry`] for
//! handwritten Rust implementations. Callers can supply a vanilla random stream
//! directly or use [`crate::world::World::with_loot_random`] for world-aware selection.

mod async_entries;
mod behavior_registry;
mod container;
mod context;
mod entries;
mod lifecycle;
#[cfg(test)]
mod location_tests;
mod modifiers;
#[cfg(test)]
pub(crate) mod name_tests;
mod operation;
mod output;
mod params;
mod predicates;
pub(crate) mod random_sequences;
mod random_stream;
pub(crate) use lifecycle::LootLifecycle;
pub(crate) use random_stream::RandomStream;
mod table;
mod table_registry;
mod visitation;
mod world_random;

pub use async_entries::AsyncLootCandidate;
pub use behavior_registry::{LootTableAsyncFunction, LootTableBehaviorRegistry, LootTableFunction};
pub use container::GeneratedLoot;
pub use context::*;
pub use entries::LootCandidate;
pub use operation::{LootCompletion, PreparedLoot};
pub use output::{
    LootAsyncModifier, LootAsyncOutput, LootFuture, LootModifier, LootOutput,
    ModifiedAsyncLootOutput, with_modifier,
};
pub use params::{LootEntitySnapshot, LootParams};
pub use table::{LootTable, LootTableRef};
pub use table_registry::{LOOT_TABLES, LootTableRegistry};
pub use vanilla::vanilla_loot_tables;
pub use world_random::{LootRandomRequest, OwnedLootRandom};

use steel_utils::Identifier;

/// A pending or failed operation, including loot-backed container access.
#[derive(Clone, Debug, thiserror::Error)]
pub enum OperationError {
    /// Access must resume after this operation; this is not an empty result.
    #[error("loot access is pending")]
    Pending(LootCompletion),
    /// A chunk request or map operation failed while evaluating world data.
    #[error("loot world-data operation failed: {0}")]
    WorldData(String),
    /// A direct caller attempted a table whose implementation requires chunk I/O.
    #[error("loot table {0} requires a resumable operation")]
    RequiresResumption(Identifier),
    /// A pending operation was explicitly cancelled before completion.
    #[error("loot operation was cancelled")]
    Cancelled,
    /// The world cannot retain a pending continuation before server binding.
    #[error("world is not bound to the server job queue")]
    SchedulerUnavailable,
    /// Container data changed during generation or another access is generating it.
    #[error("container loot generation conflicted with another access")]
    ContainerChanged,
    /// The required world stream is unavailable or is already being evaluated.
    #[error("loot random source unavailable: {0}")]
    RandomUnavailable(&'static str),
    #[error("loot table {table:?} requires unsupported operation {operation}")]
    /// An operation is known but its gameplay implementation is unavailable.
    Unsupported {
        /// Table active when evaluation failed.
        table: Option<Identifier>,
        /// Unsupported operation or capability.
        operation: &'static str,
    },
    #[error("loot table {0} has no registered implementation")]
    /// No callable was registered for the requested table.
    MissingTable(Identifier),
}

/// Result of an operation that may suspend, including container and signal queries.
pub type OperationResult<T> = Result<T, OperationError>;

#[expect(clippy::unnecessary_wraps, reason = "all compiled callables share a fallible signature")]
#[expect(clippy::absolute_paths, reason = "generated references use qualified paths to avoid collisions")]
#[expect(clippy::unreadable_literal, reason = "generated numeric tokens preserve extracted vanilla values")]
#[expect(clippy::collapsible_if, reason = "generated modifier conditions stay separate from operation control flow")]
#[expect(clippy::possible_missing_else, reason = "token serialization places independent conditionals on one line; both branches must run")]
#[rustfmt::skip]
#[path = "generated/tables.rs"]
mod vanilla;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod resumption_tests;
