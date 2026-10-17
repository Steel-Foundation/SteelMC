//! Shared blockstate providers and predicates for worldgen and item transformations

use steel_registry::{Registry, feature::BlockStateData};
use steel_utils::BlockStateId;
use steel_worldgen::state_resolver::WorldgenStateResolver;

mod predicates;
mod providers;

/// Evaluates blockstate providers against a readonly level
pub(crate) struct BlockStateProviderEvaluator;

impl BlockStateProviderEvaluator {
    pub(crate) fn block_state_from_data(
        registry: &Registry,
        data: &BlockStateData,
    ) -> BlockStateId {
        WorldgenStateResolver::feature_block_state_from_data(registry, data, "block state provider")
    }
}

#[cfg(test)]
mod tests;
