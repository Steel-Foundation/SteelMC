//! Predicates for matching individual cells in a [`BlockPattern`](super::BlockPattern).
//!
//! Vanilla collapses `BlockInWorld` + `BlockStatePredicate` into a single type here.
//! Our use cases (end portal frame, wither/golem, beacon base) only need the state.

use std::sync::Arc;

use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_utils::BlockStateId;

/// Predicate for a single cell in a [`BlockPattern`](super::BlockPattern).
#[non_exhaustive]
#[derive(Clone)]
pub enum BlockPredicate {
    /// Matches any state (vanilla's `?`).
    Any,
    /// Matches any state of the given block.
    Block(BlockRef),
    /// Matches one specific state.
    State(BlockStateId),
    /// Arbitrary closure. See [`BlockPredicate::custom`].
    Fn(Arc<dyn Fn(BlockStateId) -> bool + Send + Sync>),
}

impl BlockPredicate {
    /// Builds a custom predicate from a closure.
    #[must_use]
    pub fn custom<F>(f: F) -> Self
    where
        F: Fn(BlockStateId) -> bool + Send + Sync + 'static,
    {
        Self::Fn(Arc::new(f))
    }

    #[must_use]
    pub fn matches(&self, state: BlockStateId) -> bool {
        match self {
            Self::Any => true,
            Self::Block(block) => REGISTRY
                .blocks
                .by_state_id(state)
                .is_some_and(|b| b == *block),
            Self::State(expected) => state == *expected,
            Self::Fn(f) => f(state),
        }
    }
}

impl std::fmt::Debug for BlockPredicate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Any => f.write_str("Any"),
            Self::Block(b) => f.debug_tuple("Block").field(&b.key).finish(),
            Self::State(s) => f.debug_tuple("State").field(s).finish(),
            Self::Fn(_) => f.write_str("Fn(..)"),
        }
    }
}
