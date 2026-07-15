//! Structure set data types for generated registry data.
//!
//! These are simple data containers populated by the build script from
//! the vanilla datapack JSONs. `steel-core` converts these into its
//! placement types for actual worldgen logic.

pub use crate::structure::{DimensionPadding, JigsawConfig, PoolAlias, StartHeight};
use glam::IVec3;
use steel_utils::Identifier;

/// A structure set entry from the vanilla datapack.
#[derive(Debug, Clone)]
pub struct StructureSetData {
    /// Registry key (e.g., `minecraft:villages`).
    pub key: Identifier,
    /// Weighted structure entries.
    pub structures: Vec<StructureEntryData>,
    /// Placement configuration.
    pub placement: PlacementData,
}

/// A weighted structure entry within a structure set.
#[derive(Debug, Clone)]
pub struct StructureEntryData {
    /// Structure identifier (e.g., `minecraft:village_plains`).
    pub structure: Identifier,
    /// Selection weight.
    pub weight: i32,
}

/// Placement configuration from the vanilla datapack.
#[derive(Debug, Clone)]
pub enum PlacementData {
    /// Grid-based spread placement (`minecraft:random_spread`).
    RandomSpread {
        /// Chunk spacing between grid cell centers.
        spacing: i32,
        /// Minimum chunk separation.
        separation: i32,
        /// Spread type: `"linear"` or `"triangular"`.
        spread_type: SpreadTypeData,
        /// Unique seed modifier.
        salt: i32,
        /// Generation probability (0.0–1.0). Default: 1.0.
        frequency: f32,
        /// Frequency reduction method name. Default: `"default"`.
        frequency_reduction_method: FrequencyMethodData,
        /// Exclusion zone: (`other_set` key, `chunk_count`).
        exclusion_zone: Option<ExclusionZoneData>,
        /// Block offset from the placement chunk used by `/locate`.
        locate_offset: IVec3,
    },
    /// Ring-based placement (`minecraft:concentric_rings`).
    ConcentricRings {
        /// Base distance between rings (in chunks).
        distance: i32,
        /// Positions spread per ring.
        spread: i32,
        /// Total positions.
        count: i32,
        /// Biomes that ring positions prefer to snap to.
        preferred_biomes: Vec<Identifier>,
        /// Unique seed modifier.
        salt: i32,
        /// Generation probability. Default: 1.0.
        frequency: f32,
        /// Frequency reduction method name.
        frequency_reduction_method: FrequencyMethodData,
        /// Block offset from the placement chunk used by `/locate`.
        locate_offset: IVec3,
    },
}

/// Spread type for random spread placement.
#[derive(Debug, Clone, Copy)]
pub enum SpreadTypeData {
    /// Uniform random.
    Linear,
    /// Biased toward center.
    Triangular,
}

/// Frequency reduction method identifier.
#[derive(Debug, Clone, Copy)]
pub enum FrequencyMethodData {
    /// Standard method.
    Default,
    /// Pillager outpost legacy.
    LegacyType1,
    /// Hardcoded salt legacy.
    LegacyType2,
    /// Double-precision legacy.
    LegacyType3,
}

/// Exclusion zone preventing overlap with another structure set.
#[derive(Debug, Clone)]
pub struct ExclusionZoneData {
    /// Registry key of the other structure set.
    pub other_set: Identifier,
    /// Radius in chunks.
    pub chunk_count: i32,
}
