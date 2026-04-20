//! Predicates for matching individual cells in a [`BlockPattern`](super::BlockPattern).
//!
//! Vanilla parity: `BlockInWorld` + `BlockStatePredicate` collapsed into a single
//! predicate type. Vanilla evaluates predicates against a `BlockInWorld` (lazy block
//! entity loading); we only need the state for current use cases (end portal frame,
//! wither/golem spawning, beacon base) so we evaluate against [`BlockStateId`] directly.
//! If a future predicate needs the block entity, add a new variant.

use std::sync::Arc;

use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_utils::BlockStateId;

/// Predicate for a single cell in a [`BlockPattern`](super::BlockPattern).
///
/// `#[non_exhaustive]` so we can add FFI-safe variants (e.g. an opcode-based
/// `Encoded` form for cdylib plugins) without breaking consumers that `match`
/// on this enum.
#[non_exhaustive]
#[derive(Clone)]
pub enum BlockPredicate {
    /// Matches any block state, including air. Equivalent to vanilla's `?` cell.
    Any,
    /// Matches any state of the given block.
    Block(BlockRef),
    /// Matches one specific block state.
    State(BlockStateId),
    /// Matches via an arbitrary closure. Use [`BlockPredicate::custom`] to construct.
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

    /// Evaluates the predicate against a world block state.
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
