use steel_registry::structure::StructureData;

use crate::structure::types::generation::{GenerationStub, StructureGenerationContext};
use steel_utils::random::legacy_random::LegacyRandom;

/// Vanilla's `Structure::findValidGenerationPoint`. Impls own their RNG order,
/// collision checks, and biome check.
pub trait Structure: Send + Sync {
    /// `structure` carries registry data; per-set metadata stays in placement.
    /// `rng` is a fresh `LegacyRandom` seeded with `setLargeFeatureSeed`.
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub>;
}
