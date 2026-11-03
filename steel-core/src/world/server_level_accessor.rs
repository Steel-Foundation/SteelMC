//! Level surface handed to `Mob::finalize_spawn`.
//!
//! Vanilla passes a `ServerLevelAccessor`: the `ServerLevel` for live spawns and the
//! `WorldGenRegion` when a structure template finalizes its entities. The random source
//! matters for the latter: worldgen draws come from the region random, so later
//! `level.getRandom()` consumers in the chunk see vanilla's stream position.
//!
//! Only the queries current `finalize_spawn` implementations need are exposed;
//! `getCurrentDifficultyAt` is added once a mob that reads difficulty exists.

use std::sync::Arc;

use steel_registry::biome::BiomeRef;
use steel_utils::BlockPos;
use steel_utils::random::RandomSource;
use steel_utils::random::legacy_random::LegacyRandom;

use super::World;

/// The subset of vanilla `ServerLevelAccessor` used while finalizing a spawn.
pub trait ServerLevelAccessor {
    /// Vanilla `getRandom()`.
    fn random(&mut self) -> &mut RandomSource;

    /// Vanilla `getBiome(pos)`, the `BiomeManager`-fuzzed biome at a block, or `None`
    /// when its chunk is not available.
    fn biome(&self, pos: BlockPos) -> Option<BiomeRef>;
}

/// A live world with a fresh runtime random for one spawn.
///
/// Vanilla draws from `ServerLevel.random`; live spawns are incidental gameplay
/// randomness, so an unseeded source keeps the same distributions.
pub struct LiveSpawnLevel<'a> {
    world: &'a Arc<World>,
    random: RandomSource,
}

impl<'a> LiveSpawnLevel<'a> {
    /// Wraps a live world with a freshly seeded random source.
    #[must_use]
    pub fn new(world: &'a Arc<World>) -> Self {
        Self {
            world,
            random: RandomSource::Legacy(LegacyRandom::from_seed(rand::random())),
        }
    }
}

impl ServerLevelAccessor for LiveSpawnLevel<'_> {
    fn random(&mut self) -> &mut RandomSource {
        &mut self.random
    }

    fn biome(&self, pos: BlockPos) -> Option<BiomeRef> {
        self.world.biome_at(pos)
    }
}
