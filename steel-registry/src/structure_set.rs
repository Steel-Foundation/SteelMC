//! Structure set data types for generated registry data.
//!
//! These are simple data containers populated by the build script from
//! the vanilla datapack JSONs. `steel-core` converts these into its
//! placement types for actual worldgen logic.

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
    /// Biomes where this structure can generate (resolved from biome tags).
    pub allowed_biomes: Vec<Identifier>,
    /// Y level for biome checking. `None` means use surface height.
    /// Derived from the structure's type and start_height config.
    pub biome_check_y: Option<i32>,
    /// Structure type identifier (e.g., `"minecraft:jigsaw"`, `"minecraft:mineshaft"`).
    pub structure_type: String,
    /// Jigsaw-specific configuration. Present only for `minecraft:jigsaw` structures.
    pub jigsaw_config: Option<JigsawConfig>,
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
        /// Exclusion zone: (other_set key, chunk_count).
        exclusion_zone: Option<ExclusionZoneData>,
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
    },
}

/// Configuration for a jigsaw structure, parsed from its structure JSON.
#[derive(Debug, Clone)]
pub struct JigsawConfig {
    /// Starting template pool.
    pub start_pool: Identifier,
    /// Maximum recursion depth (vanilla calls this `size`).
    pub max_depth: i32,
    /// Whether the expansion hack is enabled.
    pub use_expansion_hack: bool,
    /// If set, project the start piece to this heightmap type.
    pub project_start_to_heightmap: Option<String>,
    /// Start height provider type and value.
    pub start_height: StartHeight,
    /// Maximum distance from center for piece placement.
    pub max_distance_from_center: i32,
    /// Optional named jigsaw to anchor the start piece to.
    pub start_jigsaw_name: Option<Identifier>,
    /// Dimension padding (min distance from world height limits).
    pub dimension_padding: DimensionPadding,
    /// Terrain adaptation mode.
    pub terrain_adaptation: String,
    /// Pool alias configurations.
    pub pool_aliases: Vec<PoolAlias>,
}

/// Start height configuration.
#[derive(Debug, Clone)]
pub enum StartHeight {
    /// Fixed absolute Y.
    Constant(i32),
    /// Uniform random between min and max (inclusive).
    Uniform { min: i32, max: i32 },
}

/// Dimension padding (how close pieces can be to world height limits).
#[derive(Debug, Clone, Copy)]
pub struct DimensionPadding {
    /// Bottom padding.
    pub bottom: i32,
    /// Top padding.
    pub top: i32,
}

/// A pool alias remapping.
#[derive(Debug, Clone)]
pub enum PoolAlias {
    /// Direct remapping: alias -> target.
    Direct {
        alias: Identifier,
        target: Identifier,
    },
    /// Random selection from weighted targets.
    Random {
        alias: Identifier,
        targets: Vec<(Identifier, i32)>,
    },
    /// Random group: pick one group, apply all bindings in it.
    RandomGroup {
        groups: Vec<(Vec<(Identifier, Identifier)>, i32)>,
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
