use std::sync::OnceLock;

use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// A registered worldgen structure definition.
///
/// Mirrors vanilla's `Structure`: common settings are stored here, while
/// [`StructureConfigData`] carries the type-specific codec payload.
#[derive(Debug)]
pub struct StructureData {
    /// Registry key, e.g. `minecraft:village_plains`.
    pub key: Identifier,
    /// Cached registry ID, set during registration for O(1) lookup on hot paths.
    pub id: OnceLock<usize>,
    /// Structure type, e.g. `minecraft:jigsaw` or `minecraft:mineshaft`.
    pub structure_type: Identifier,
    /// Biomes this structure can generate in. Tags are resolved at build time.
    pub allowed_biomes: Vec<Identifier>,
    /// Structure-specific mob spawn overrides.
    pub spawn_overrides: Vec<StructureSpawnOverrideData>,
    /// Generation decoration step from the structure JSON.
    pub step: StructureGenerationStep,
    /// Terrain adaptation used by reference inflation and Beardifier.
    pub terrain_adjustment: TerrainAdjustment,
    /// Type-specific structure config.
    pub config: StructureConfigData,
}

impl StructureData {
    /// Vanilla inflates the structure start bounding box by 12 for every terrain
    /// adaptation mode except `none`.
    #[must_use]
    pub const fn bb_inflate(&self) -> i32 {
        self.terrain_adjustment.bb_inflate()
    }
}

pub type StructureRef = &'static StructureData;

/// Registry of worldgen structure definitions.
pub struct StructureRegistry {
    structures_by_id: Vec<StructureRef>,
    structures_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl StructureRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            structures_by_id: Vec::new(),
            structures_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, entry: StructureRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register StructureData after registry has been frozen"
        );
        let id = self.structures_by_id.len();
        let cached = entry.id.get_or_init(|| id);
        assert_eq!(*cached, id, "structure registered with conflicting id");
        self.structures_by_id.push(entry);
        self.structures_by_key.insert(entry.key.clone(), id);
        id
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, StructureRef)> + '_ {
        self.structures_by_id
            .iter()
            .enumerate()
            .map(|(id, &entry)| (id, entry))
    }
}

impl Default for StructureRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry_ext!(
    StructureRegistry,
    StructureData,
    structures_by_id,
    structures_by_key
);
crate::impl_tagged_registry!(StructureRegistry, structures_by_key, "structure");

crate::impl_registry_entry_eq!(StructureData);

impl crate::RegistryEntry for StructureData {
    fn key(&self) -> &Identifier {
        &self.key
    }

    fn try_id(&self) -> Option<usize> {
        self.id.get().copied()
    }
}

/// Structure generation step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureGenerationStep {
    /// `surface_structures`.
    SurfaceStructures,
    /// `underground_structures`.
    UndergroundStructures,
    /// `underground_decoration`.
    UndergroundDecoration,
}

impl StructureGenerationStep {
    /// Decoration-stage ordinal used by vanilla `GenerationStep.Decoration`.
    ///
    /// Structure JSON only names the three structure-capable decoration stages;
    /// feature generation still runs all eleven decoration stages, so these
    /// values intentionally leave the vanilla gaps intact.
    #[must_use]
    pub const fn decoration_ordinal(self) -> usize {
        match self {
            Self::UndergroundStructures => 3,
            Self::SurfaceStructures => 4,
            Self::UndergroundDecoration => 7,
        }
    }
}

/// How a structure modifies surrounding terrain.
///
/// Corresponds to vanilla's `TerrainAdjustment` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainAdjustment {
    /// No terrain adaptation.
    None,
    /// Fill in terrain around and above the structure.
    Bury,
    /// Carve thin beard below structure.
    BeardThin,
    /// Carve box-shaped beard below structure.
    BeardBox,
    /// Encapsulate structure in terrain.
    Encapsulate,
}

impl TerrainAdjustment {
    /// Bounding-box inflation used by vanilla's `Structure.adjustBoundingBox`.
    #[must_use]
    pub const fn bb_inflate(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Bury | Self::BeardThin | Self::BeardBox | Self::Encapsulate => 12,
        }
    }
}

/// Spawn override bounding-box mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureSpawnBoundingBox {
    /// Applies to the whole structure start bounding box.
    Full,
    /// Applies only when inside one of the pieces.
    Piece,
}

#[cfg(test)]
mod tests {
    use crate::{Registry, TaggedRegistryExt};

    use super::*;

    #[test]
    fn vanilla_structure_tags_are_registered() {
        let registry = Registry::new_vanilla();
        let village_tag = Identifier::vanilla_static("village");
        let villages = registry.structures.get_tag(&village_tag);
        assert!(villages.as_ref().is_some_and(|entries| {
            entries
                .iter()
                .any(|structure| structure.key == Identifier::vanilla_static("village_plains"))
        }));
    }

    #[test]
    fn structure_generation_steps_use_vanilla_decoration_ordinals() {
        assert_eq!(
            StructureGenerationStep::UndergroundStructures.decoration_ordinal(),
            3
        );
        assert_eq!(
            StructureGenerationStep::SurfaceStructures.decoration_ordinal(),
            4
        );
        assert_eq!(
            StructureGenerationStep::UndergroundDecoration.decoration_ordinal(),
            7
        );
    }
}

/// A structure mob spawn override for one mob category.
#[derive(Debug, Clone)]
pub struct StructureSpawnOverrideData {
    /// Mob category name, e.g. `monster`.
    pub category: String,
    /// Bounding box mode.
    pub bounding_box: StructureSpawnBoundingBox,
    /// Weighted spawns for this override.
    pub spawns: Vec<StructureSpawnerData>,
}

/// Spawn entry inside a structure spawn override.
#[derive(Debug, Clone)]
pub struct StructureSpawnerData {
    /// Entity type id.
    pub entity_type: Identifier,
    /// Spawn weight.
    pub weight: i32,
    /// Minimum group size.
    pub min_count: i32,
    /// Maximum group size.
    pub max_count: i32,
}

/// Type-specific structure config.
#[derive(Debug, Clone)]
pub enum StructureConfigData {
    /// `minecraft:jigsaw`.
    Jigsaw(JigsawConfig),
    /// `minecraft:mineshaft`.
    Mineshaft { mineshaft_type: MineshaftTypeData },
    /// `minecraft:shipwreck`.
    Shipwreck { is_beached: bool },
    /// `minecraft:ocean_ruin`.
    OceanRuin {
        biome_temp: OceanRuinBiomeTempData,
        large_probability: f32,
        cluster_probability: f32,
    },
    /// `minecraft:ruined_portal`.
    RuinedPortal { setups: Vec<RuinedPortalSetupData> },
    /// `minecraft:nether_fossil`.
    NetherFossil { height: HeightProviderData },
    /// Structure types with only common settings, or whose config is still unused.
    Empty,
}

impl StructureConfigData {
    #[must_use]
    pub fn as_jigsaw(&self) -> Option<&JigsawConfig> {
        match self {
            Self::Jigsaw(config) => Some(config),
            _ => None,
        }
    }
}

/// Jigsaw-specific configuration parsed from structure JSON.
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
    /// Pool alias configurations.
    pub pool_aliases: Vec<PoolAlias>,
    /// Liquid handling mode.
    pub liquid_settings: LiquidSettingsData,
}

/// Start height configuration used by currently-generated jigsaw structures.
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

/// Jigsaw liquid handling mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiquidSettingsData {
    /// Default vanilla behavior.
    ApplyWaterlogging,
    /// Do not apply waterlogging from surrounding fluids.
    IgnoreWaterlogging,
}

/// Mineshaft variant type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MineshaftTypeData {
    /// Standard mineshaft.
    Normal,
    /// Badlands mineshaft.
    Mesa,
}

/// Ocean ruin temperature variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OceanRuinBiomeTempData {
    /// Warm ruin pools.
    Warm,
    /// Cold ruin pools.
    Cold,
}

/// Ruined portal vertical placement type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuinedPortalPlacementData {
    OnLandSurface,
    PartlyBuried,
    Underground,
    InMountain,
    OnOceanFloor,
    InNether,
}

/// One weighted ruined portal setup entry.
#[derive(Debug, Clone)]
pub struct RuinedPortalSetupData {
    pub placement: RuinedPortalPlacementData,
    pub weight: f32,
    pub air_pocket_probability: f32,
    pub can_be_cold: bool,
    pub mossiness: f32,
    pub overgrown: bool,
    pub replace_with_blackstone: bool,
    pub vines: bool,
}

/// Generic vertical anchor used by non-jigsaw height providers.
#[derive(Debug, Clone)]
pub enum VerticalAnchorData {
    Absolute(i32),
    AboveBottom(i32),
    BelowTop(i32),
}

/// Height provider subset used by vanilla structures in this version.
#[derive(Debug, Clone)]
pub enum HeightProviderData {
    Constant(VerticalAnchorData),
    Uniform {
        min_inclusive: VerticalAnchorData,
        max_inclusive: VerticalAnchorData,
    },
}
