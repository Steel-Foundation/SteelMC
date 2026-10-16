//! Loot tables compiled from vanilla data into Rust functions.
//!
//! See [`LootTable`] for generation and [`LootTableBehaviorRegistry`] for
//! handwritten Rust implementations. Callers can supply a vanilla random stream
//! directly or use [`crate::world::World::with_loot_random`] for world-aware selection.

mod behavior_registry;
mod container;
mod context;
mod entries;
mod modifiers;
mod output;
mod predicates;
pub(crate) mod random_sequences;
mod table;
mod table_registry;
mod world_random;

pub use behavior_registry::{LootTableBehaviorRegistry, LootTableFunction};
pub use container::GeneratedLoot;
pub use context::*;
pub use entries::LootCandidate;
pub use output::{LootModifier, LootOutput, with_modifier};
pub use table::{LootTable, LootTableRef};
pub use table_registry::{LOOT_TABLES, LootTableRegistry};
pub use vanilla::vanilla_loot_tables;

use steel_utils::Identifier;

/// A recoverable failure evaluating a compiled loot operation.
#[derive(Debug, thiserror::Error)]
pub enum LootError {
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

/// Result of evaluating a compiled table, condition, or modifier.
pub type LootResult<T> = Result<T, LootError>;

#[expect(clippy::unnecessary_wraps, reason = "all compiled callables share a fallible signature")]
#[expect(clippy::absolute_paths, reason = "generated references use qualified paths to avoid collisions")]
#[expect(clippy::unreadable_literal, reason = "generated numeric tokens preserve extracted vanilla values")]
#[expect(clippy::collapsible_if, reason = "generated modifier conditions stay separate from operation control flow")]
#[expect(clippy::too_many_lines, reason = "generated pool sizes follow extracted entries, which expand inline")]
#[expect(clippy::vec_init_then_push, reason = "generated entries append candidates in order, including conditional and composite expansions")]
#[rustfmt::skip]
#[path = "generated/tables.rs"]
mod vanilla;

#[cfg(test)]
mod tests;
