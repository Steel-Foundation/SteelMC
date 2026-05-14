pub(super) use std::sync::LazyLock;

pub(super) use rustc_hash::{FxHashMap, FxHashSet};
pub(super) use steel_registry::blocks::{
    BlockRef, block_state_ext::BlockStateExt as _, properties::BlockStateProperties,
    properties::DoubleBlockHalf, properties::EnumProperty, properties::WallSide, shapes,
};
pub(super) use steel_registry::feature::{
    BlockBlobConfiguration, BlockColumnConfiguration, BlockPileConfiguration, BlockPredicate,
    BlockStateData, BlockStateProvider, ConfiguredFeatureKind, ConfiguredFeatureRef,
    DiskConfiguration, DualNoiseProvider, FeatureHeightmap, FeatureNoiseParameters, FluidStateData,
    IdentifierList, NoiseProvider, NoiseThresholdProvider, OreConfiguration, OreTarget,
    PlacedFeatureData, PlacedFeatureEntryRef, PlacedFeatureRef, PlacementModifier, RuleTest,
    SimpleBlockConfiguration, SpringConfiguration,
};
pub(super) use steel_registry::fluid::{FluidRef, FluidState, FluidStateExt as _};
pub(super) use steel_registry::{
    Registry, RegistryEntry as _, RegistryExt as _, TaggedRegistryExt as _, vanilla_blocks,
};
pub(super) use steel_utils::math::Axis;
pub(super) use steel_utils::random::{
    Random as _, RandomSource, legacy_random::LegacyRandom, xoroshiro::Xoroshiro,
};
pub(super) use steel_utils::types::UpdateFlags;
pub(super) use steel_utils::value_providers::IntProvider;
pub(super) use steel_utils::{BlockPos, BlockStateId, Direction, Identifier, SectionPos};
pub(super) use steel_worldgen::math::{floor, lerp};
pub(super) use steel_worldgen::noise::{NormalNoise, PerlinSimplexNoise};

pub(super) use crate::behavior::BLOCK_BEHAVIORS;
pub(super) use crate::chunk::chunk_access::ChunkStatus;
pub(super) use crate::chunk::heightmap::HeightmapType;
pub(super) use crate::fluid::state::get_fluid_state_from_block;
pub(super) use crate::worldgen::generators::vanilla::fuzzed_biome_at_block;
pub(super) use crate::worldgen::region::{WorldGenBulkSectionAccess, WorldGenRegion};

pub(super) const DECORATION_STEP_COUNT: usize = 11;
