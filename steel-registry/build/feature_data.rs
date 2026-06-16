//! Build-time JSON codec for vanilla worldgen feature registry data.
//!
//! These types mirror vanilla's configured feature, placed feature, placement
//! modifier, provider, predicate, and tree-shape codec data. Runtime feature
//! data in `src/feature/data.rs` can use typed registry refs because this module
//! owns the extracted JSON decoding step.

use crate::shared_structs::deserialize_tag_identifier;
pub use crate::shared_structs::{BlockStateData, FluidStateData};
use serde::{Deserialize, Deserializer, de::Error as _};
use serde_json::Value;
use steel_utils::{
    Direction, Identifier, Rotation,
    value_providers::{FloatProvider, HeightProvider, IntProvider, UniformIntProvider},
};

/// A configured feature reference, either a registry key or an inline configured feature.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ConfiguredFeatureRef {
    /// Registry-backed configured feature.
    Reference(Identifier),
    /// Inline configured feature.
    Inline(Box<ConfiguredFeatureKind>),
}

/// A placed feature reference, either a registry key or an inline placed feature.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PlacedFeatureRef {
    /// Registry-backed placed feature.
    Reference(Identifier),
    /// Inline placed feature.
    Inline(Box<PlacedFeatureData>),
}

/// A placed feature: configured feature plus ordered placement modifiers.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedFeatureData {
    /// Configured feature reference.
    pub feature: ConfiguredFeatureRef,
    /// Ordered placement modifier chain.
    #[serde(default)]
    pub placement: Vec<PlacementModifier>,
}

/// A configured feature kind with its typed configuration.
#[derive(Debug, Clone)]
#[expect(
    clippy::large_enum_variant,
    reason = "typed feature configs are registry data moved by reference; boxing individual variants would add noise before placement implementations use them"
)]
pub enum ConfiguredFeatureKind {
    Bamboo(BambooConfiguration),
    BasaltColumns(BasaltColumnsConfiguration),
    BasaltPillar,
    BlockBlob(BlockBlobConfiguration),
    BlockColumn(BlockColumnConfiguration),
    BlockPile(BlockPileConfiguration),
    BlueIce,
    BonusChest,
    ChorusPlant,
    CoralClaw,
    CoralMushroom,
    CoralTree,
    DeltaFeature(DeltaFeatureConfiguration),
    DesertWell,
    Disk(DiskConfiguration),
    DripstoneCluster(DripstoneClusterConfiguration),
    EndGateway(EndGatewayConfiguration),
    EndIsland,
    EndPlatform,
    EndSpike(EndSpikeConfiguration),
    FallenTree(FallenTreeConfiguration),
    Fossil(FossilConfiguration),
    FreezeTopLayer,
    Geode(GeodeConfiguration),
    GlowstoneBlob,
    HugeBrownMushroom(HugeMushroomConfiguration),
    HugeFungus(HugeFungusConfiguration),
    HugeRedMushroom(HugeMushroomConfiguration),
    Iceberg(BlockStateData),
    Kelp,
    Lake(LakeConfiguration),
    LargeDripstone(LargeDripstoneConfiguration),
    MonsterRoom,
    MultifaceGrowth(MultifaceGrowthConfiguration),
    NetherForestVegetation(NetherForestVegetationConfiguration),
    NetherrackReplaceBlobs(NetherrackReplaceBlobsConfiguration),
    Ore(OreConfiguration),
    PointedDripstone(PointedDripstoneConfiguration),
    RandomBooleanSelector(RandomBooleanSelectorConfiguration),
    RandomSelector(RandomSelectorConfiguration),
    RootSystem(RootSystemConfiguration),
    ScatteredOre(OreConfiguration),
    SculkPatch(SculkPatchConfiguration),
    SeaPickle(SeaPickleConfiguration),
    Seagrass(SeagrassConfiguration),
    Sequence(CompositeFeatureConfiguration),
    SimpleBlock(SimpleBlockConfiguration),
    SimpleRandomSelector(SimpleRandomSelectorConfiguration),
    Speleothem(SpeleothemConfiguration),
    SpeleothemCluster(SpeleothemClusterConfiguration),
    Spike(SpikeConfiguration),
    SpringFeature(SpringConfiguration),
    Template(TemplateFeatureConfiguration),
    Tree(TreeConfiguration),
    TwistingVines(TwistingVinesConfiguration),
    UnderwaterMagma(UnderwaterMagmaConfiguration),
    VegetationPatch(VegetationPatchConfiguration),
    Vines,
    VoidStartPlatform,
    WaterloggedVegetationPatch(VegetationPatchConfiguration),
    WeightedRandomSelector(WeightedRandomFeatureConfiguration),
    WeepingVines,
}

impl<'de> Deserialize<'de> for ConfiguredFeatureKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            #[serde(rename = "type")]
            feature_type: Identifier,
            #[serde(default)]
            config: Value,
        }

        let raw = Raw::deserialize(deserializer)?;
        deserialize_configured_feature_kind(raw.feature_type, raw.config).map_err(D::Error::custom)
    }
}

fn deserialize_configured_feature_kind(
    feature_type: Identifier,
    config: Value,
) -> Result<ConfiguredFeatureKind, String> {
    macro_rules! parse {
        ($ty:ty) => {
            serde_json::from_value::<$ty>(config).map_err(|err| err.to_string())
        };
    }

    Ok(match feature_type.to_string().as_str() {
        "minecraft:bamboo" => ConfiguredFeatureKind::Bamboo(parse!(BambooConfiguration)?),
        "minecraft:basalt_columns" => {
            ConfiguredFeatureKind::BasaltColumns(parse!(BasaltColumnsConfiguration)?)
        }
        "minecraft:basalt_pillar" => ConfiguredFeatureKind::BasaltPillar,
        "minecraft:block_blob" => ConfiguredFeatureKind::BlockBlob(parse!(BlockBlobConfiguration)?),
        "minecraft:block_column" => {
            ConfiguredFeatureKind::BlockColumn(parse!(BlockColumnConfiguration)?)
        }
        "minecraft:block_pile" => ConfiguredFeatureKind::BlockPile(parse!(BlockPileConfiguration)?),
        "minecraft:blue_ice" => ConfiguredFeatureKind::BlueIce,
        "minecraft:bonus_chest" => ConfiguredFeatureKind::BonusChest,
        "minecraft:chorus_plant" => ConfiguredFeatureKind::ChorusPlant,
        "minecraft:coral_claw" => ConfiguredFeatureKind::CoralClaw,
        "minecraft:coral_mushroom" => ConfiguredFeatureKind::CoralMushroom,
        "minecraft:coral_tree" => ConfiguredFeatureKind::CoralTree,
        "minecraft:delta_feature" => {
            ConfiguredFeatureKind::DeltaFeature(parse!(DeltaFeatureConfiguration)?)
        }
        "minecraft:desert_well" => ConfiguredFeatureKind::DesertWell,
        "minecraft:disk" => ConfiguredFeatureKind::Disk(parse!(DiskConfiguration)?),
        "minecraft:dripstone_cluster" => {
            ConfiguredFeatureKind::DripstoneCluster(parse!(DripstoneClusterConfiguration)?)
        }
        "minecraft:end_gateway" => {
            ConfiguredFeatureKind::EndGateway(parse!(EndGatewayConfiguration)?)
        }
        "minecraft:end_island" => ConfiguredFeatureKind::EndIsland,
        "minecraft:end_platform" => ConfiguredFeatureKind::EndPlatform,
        "minecraft:end_spike" => ConfiguredFeatureKind::EndSpike(parse!(EndSpikeConfiguration)?),
        "minecraft:fallen_tree" => {
            ConfiguredFeatureKind::FallenTree(parse!(FallenTreeConfiguration)?)
        }
        "minecraft:fossil" => ConfiguredFeatureKind::Fossil(parse!(FossilConfiguration)?),
        "minecraft:freeze_top_layer" => ConfiguredFeatureKind::FreezeTopLayer,
        "minecraft:geode" => ConfiguredFeatureKind::Geode(parse!(GeodeConfiguration)?),
        "minecraft:glowstone_blob" => ConfiguredFeatureKind::GlowstoneBlob,
        "minecraft:huge_brown_mushroom" => {
            ConfiguredFeatureKind::HugeBrownMushroom(parse!(HugeMushroomConfiguration)?)
        }
        "minecraft:huge_fungus" => {
            ConfiguredFeatureKind::HugeFungus(parse!(HugeFungusConfiguration)?)
        }
        "minecraft:huge_red_mushroom" => {
            ConfiguredFeatureKind::HugeRedMushroom(parse!(HugeMushroomConfiguration)?)
        }
        "minecraft:iceberg" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct IcebergConfig {
                state: BlockStateData,
            }
            ConfiguredFeatureKind::Iceberg(parse!(IcebergConfig)?.state)
        }
        "minecraft:kelp" => ConfiguredFeatureKind::Kelp,
        "minecraft:lake" => ConfiguredFeatureKind::Lake(parse!(LakeConfiguration)?),
        "minecraft:large_dripstone" => {
            ConfiguredFeatureKind::LargeDripstone(parse!(LargeDripstoneConfiguration)?)
        }
        "minecraft:monster_room" => ConfiguredFeatureKind::MonsterRoom,
        "minecraft:multiface_growth" => {
            ConfiguredFeatureKind::MultifaceGrowth(parse!(MultifaceGrowthConfiguration)?)
        }
        "minecraft:nether_forest_vegetation" => ConfiguredFeatureKind::NetherForestVegetation(
            parse!(NetherForestVegetationConfiguration)?,
        ),
        "minecraft:netherrack_replace_blobs" => ConfiguredFeatureKind::NetherrackReplaceBlobs(
            parse!(NetherrackReplaceBlobsConfiguration)?,
        ),
        "minecraft:ore" => ConfiguredFeatureKind::Ore(parse!(OreConfiguration)?),
        "minecraft:pointed_dripstone" => {
            ConfiguredFeatureKind::PointedDripstone(parse!(PointedDripstoneConfiguration)?)
        }
        "minecraft:random_boolean_selector" => ConfiguredFeatureKind::RandomBooleanSelector(
            parse!(RandomBooleanSelectorConfiguration)?,
        ),
        "minecraft:random_selector" => {
            ConfiguredFeatureKind::RandomSelector(parse!(RandomSelectorConfiguration)?)
        }
        "minecraft:weighted_random_selector" => ConfiguredFeatureKind::WeightedRandomSelector(
            parse!(WeightedRandomFeatureConfiguration)?,
        ),
        "minecraft:root_system" => {
            ConfiguredFeatureKind::RootSystem(parse!(RootSystemConfiguration)?)
        }
        "minecraft:scattered_ore" => ConfiguredFeatureKind::ScatteredOre(parse!(OreConfiguration)?),
        "minecraft:sculk_patch" => {
            ConfiguredFeatureKind::SculkPatch(parse!(SculkPatchConfiguration)?)
        }
        "minecraft:sea_pickle" => ConfiguredFeatureKind::SeaPickle(parse!(SeaPickleConfiguration)?),
        "minecraft:seagrass" => ConfiguredFeatureKind::Seagrass(parse!(SeagrassConfiguration)?),
        "minecraft:sequence" => {
            ConfiguredFeatureKind::Sequence(parse!(CompositeFeatureConfiguration)?)
        }
        "minecraft:simple_block" => {
            ConfiguredFeatureKind::SimpleBlock(parse!(SimpleBlockConfiguration)?)
        }
        "minecraft:simple_random_selector" => {
            ConfiguredFeatureKind::SimpleRandomSelector(parse!(SimpleRandomSelectorConfiguration)?)
        }
        "minecraft:speleothem" => {
            ConfiguredFeatureKind::Speleothem(parse!(SpeleothemConfiguration)?)
        }
        "minecraft:speleothem_cluster" => {
            ConfiguredFeatureKind::SpeleothemCluster(parse!(SpeleothemClusterConfiguration)?)
        }
        "minecraft:spike" => ConfiguredFeatureKind::Spike(parse!(SpikeConfiguration)?),
        "minecraft:spring_feature" => {
            ConfiguredFeatureKind::SpringFeature(parse!(SpringConfiguration)?)
        }
        "minecraft:template" => {
            ConfiguredFeatureKind::Template(parse!(TemplateFeatureConfiguration)?)
        }
        "minecraft:tree" => ConfiguredFeatureKind::Tree(parse!(TreeConfiguration)?),
        "minecraft:twisting_vines" => {
            ConfiguredFeatureKind::TwistingVines(parse!(TwistingVinesConfiguration)?)
        }
        "minecraft:underwater_magma" => {
            ConfiguredFeatureKind::UnderwaterMagma(parse!(UnderwaterMagmaConfiguration)?)
        }
        "minecraft:vegetation_patch" => {
            ConfiguredFeatureKind::VegetationPatch(parse!(VegetationPatchConfiguration)?)
        }
        "minecraft:vines" => ConfiguredFeatureKind::Vines,
        "minecraft:void_start_platform" => ConfiguredFeatureKind::VoidStartPlatform,
        "minecraft:waterlogged_vegetation_patch" => {
            ConfiguredFeatureKind::WaterloggedVegetationPatch(parse!(VegetationPatchConfiguration)?)
        }
        "minecraft:weeping_vines" => ConfiguredFeatureKind::WeepingVines,
        other => return Err(format!("unknown configured feature type `{other}`")),
    })
}

/// Identifier list that accepts vanilla's single-or-list codec shape.
#[derive(Debug, Clone)]
pub struct IdentifierList(pub Vec<Identifier>);

impl<'de> Deserialize<'de> for IdentifierList {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Single(Identifier),
            Many(Vec<Identifier>),
        }

        Ok(match Raw::deserialize(deserializer)? {
            Raw::Single(value) => Self(vec![value]),
            Raw::Many(values) => Self(values),
        })
    }
}

/// Vanilla holder set for blocks, preserving tag-vs-entry semantics.
#[derive(Debug, Clone)]
pub enum BlockHolderSet {
    Tag(Identifier),
    Entries(Vec<Identifier>),
}

impl<'de> Deserialize<'de> for BlockHolderSet {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Single(String),
            Many(Vec<Identifier>),
        }

        match Raw::deserialize(deserializer)? {
            Raw::Single(value) => {
                if let Some(tag) = value.strip_prefix('#') {
                    let tag = tag.parse().map_err(D::Error::custom)?;
                    Ok(Self::Tag(tag))
                } else {
                    let entry = value.parse().map_err(D::Error::custom)?;
                    Ok(Self::Entries(vec![entry]))
                }
            }
            Raw::Many(values) => Ok(Self::Entries(values)),
        }
    }
}

/// Block position offset.
pub type Offset = [i32; 3];

fn default_offset() -> Offset {
    [0, 0, 0]
}

fn deserialize_direction<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Direction, D::Error> {
    let value = String::deserialize(deserializer)?;
    parse_direction(&value).map_err(D::Error::custom)
}

fn deserialize_directions<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Direction>, D::Error> {
    let values = Vec::<String>::deserialize(deserializer)?;
    values
        .iter()
        .map(|value| parse_direction(value).map_err(D::Error::custom))
        .collect()
}

fn parse_direction(value: &str) -> Result<Direction, &'static str> {
    match value {
        "down" => Ok(Direction::Down),
        "up" => Ok(Direction::Up),
        "north" => Ok(Direction::North),
        "south" => Ok(Direction::South),
        "west" => Ok(Direction::West),
        "east" => Ok(Direction::East),
        _ => Err("invalid direction"),
    }
}

/// Block predicates used by placement modifiers and feature configs.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum BlockPredicate {
    #[serde(rename = "minecraft:true")]
    True,
    #[serde(rename = "minecraft:all_of")]
    AllOf { predicates: Vec<BlockPredicate> },
    #[serde(rename = "minecraft:any_of")]
    AnyOf { predicates: Vec<BlockPredicate> },
    #[serde(rename = "minecraft:not")]
    Not { predicate: Box<BlockPredicate> },
    #[serde(rename = "minecraft:matching_block_tag")]
    MatchingBlockTag {
        tag: Identifier,
        #[serde(default = "default_offset")]
        offset: Offset,
    },
    #[serde(rename = "minecraft:matching_blocks")]
    MatchingBlocks {
        blocks: IdentifierList,
        #[serde(default = "default_offset")]
        offset: Offset,
    },
    #[serde(rename = "minecraft:matching_fluids")]
    MatchingFluids {
        fluids: IdentifierList,
        #[serde(default = "default_offset")]
        offset: Offset,
    },
    #[serde(rename = "minecraft:solid")]
    Solid {
        #[serde(default = "default_offset")]
        offset: Offset,
    },
    #[serde(rename = "minecraft:would_survive")]
    WouldSurvive {
        state: BlockStateData,
        #[serde(default = "default_offset")]
        offset: Offset,
    },
    #[serde(rename = "minecraft:replaceable")]
    Replaceable {
        #[serde(default = "default_offset")]
        offset: Offset,
    },
    #[serde(rename = "minecraft:has_sturdy_face")]
    HasSturdyFace {
        #[serde(deserialize_with = "deserialize_direction")]
        direction: Direction,
        #[serde(default = "default_offset")]
        offset: Offset,
    },
    #[serde(rename = "minecraft:inside_world_bounds")]
    InsideWorldBounds {
        #[serde(default = "default_offset")]
        offset: Offset,
    },
}

/// Block-state provider used by features.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum BlockStateProvider {
    #[serde(rename = "minecraft:simple_state_provider")]
    Simple { state: BlockStateData },
    #[serde(rename = "minecraft:weighted_state_provider")]
    Weighted { entries: Vec<WeightedBlockState> },
    #[serde(rename = "minecraft:rotated_block_provider")]
    RotatedBlock { state: BlockStateData },
    #[serde(rename = "minecraft:randomized_int_state_provider")]
    RandomizedInt {
        property: String,
        source: Box<BlockStateProvider>,
        values: IntProvider,
    },
    #[serde(rename = "minecraft:rule_based_state_provider")]
    RuleBased {
        #[serde(default)]
        fallback: Option<Box<BlockStateProvider>>,
        rules: Vec<RuleBasedStateProviderRule>,
    },
    #[serde(rename = "minecraft:noise_provider")]
    Noise(NoiseProvider),
    #[serde(rename = "minecraft:noise_threshold_provider")]
    NoiseThreshold(NoiseThresholdProvider),
    #[serde(rename = "minecraft:dual_noise_provider")]
    DualNoise(DualNoiseProvider),
}

/// Weighted block-state provider entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightedBlockState {
    pub data: BlockStateData,
    pub weight: i32,
}

/// Rule-based provider rule.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleBasedStateProviderRule {
    pub if_true: BlockPredicate,
    pub then: BlockStateProvider,
}

/// Noise parameters embedded in vanilla feature providers.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureNoiseParameters {
    #[serde(rename = "firstOctave")]
    pub first_octave: i32,
    pub amplitudes: Vec<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoiseProvider {
    pub noise: FeatureNoiseParameters,
    pub scale: f32,
    pub seed: i64,
    pub states: Vec<BlockStateData>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoiseThresholdProvider {
    pub noise: FeatureNoiseParameters,
    pub scale: f32,
    pub seed: i64,
    pub threshold: f32,
    pub high_chance: f32,
    pub default_state: BlockStateData,
    pub low_states: Vec<BlockStateData>,
    pub high_states: Vec<BlockStateData>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DualNoiseProvider {
    pub noise: FeatureNoiseParameters,
    pub scale: f32,
    pub seed: i64,
    pub slow_noise: FeatureNoiseParameters,
    pub slow_scale: f32,
    pub states: Vec<BlockStateData>,
    pub variety: [i32; 2],
}

/// Feature placement modifiers.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum PlacementModifier {
    #[serde(rename = "minecraft:biome")]
    Biome,
    #[serde(rename = "minecraft:block_predicate_filter")]
    BlockPredicateFilter { predicate: BlockPredicate },
    #[serde(rename = "minecraft:count")]
    Count { count: IntProvider },
    #[serde(rename = "minecraft:count_on_every_layer")]
    CountOnEveryLayer { count: IntProvider },
    #[serde(rename = "minecraft:environment_scan")]
    EnvironmentScan {
        #[serde(deserialize_with = "deserialize_direction")]
        direction_of_search: Direction,
        target_condition: BlockPredicate,
        #[serde(default)]
        allowed_search_condition: Option<BlockPredicate>,
        max_steps: i32,
    },
    #[serde(rename = "minecraft:fixed_placement")]
    FixedPlacement { positions: Vec<Offset> },
    #[serde(rename = "minecraft:height_range")]
    HeightRange { height: HeightProvider },
    #[serde(rename = "minecraft:heightmap")]
    Heightmap { heightmap: FeatureHeightmap },
    #[serde(rename = "minecraft:in_square")]
    InSquare,
    #[serde(rename = "minecraft:noise_based_count")]
    NoiseBasedCount {
        noise_to_count_ratio: i32,
        noise_factor: f64,
        #[serde(default)]
        noise_offset: f64,
    },
    #[serde(rename = "minecraft:noise_threshold_count")]
    NoiseThresholdCount {
        noise_level: f64,
        below_noise: i32,
        above_noise: i32,
    },
    #[serde(rename = "minecraft:random_offset")]
    RandomOffset {
        xz_spread: IntProvider,
        y_spread: IntProvider,
    },
    #[serde(rename = "minecraft:rarity_filter")]
    RarityFilter { chance: i32 },
    #[serde(rename = "minecraft:surface_relative_threshold_filter")]
    SurfaceRelativeThresholdFilter {
        heightmap: FeatureHeightmap,
        #[serde(default)]
        min_inclusive: Option<i32>,
        #[serde(default)]
        max_inclusive: Option<i32>,
    },
    #[serde(rename = "minecraft:surface_water_depth_filter")]
    SurfaceWaterDepthFilter { max_water_depth: i32 },
}

/// Heightmap names used by placed feature modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureHeightmap {
    WorldSurface,
    MotionBlocking,
    MotionBlockingNoLeaves,
    OceanFloor,
    WorldSurfaceWg,
    OceanFloorWg,
}

impl<'de> Deserialize<'de> for FeatureHeightmap {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "WORLD_SURFACE" => Self::WorldSurface,
            "MOTION_BLOCKING" => Self::MotionBlocking,
            "MOTION_BLOCKING_NO_LEAVES" => Self::MotionBlockingNoLeaves,
            "OCEAN_FLOOR" => Self::OceanFloor,
            "WORLD_SURFACE_WG" => Self::WorldSurfaceWg,
            "OCEAN_FLOOR_WG" => Self::OceanFloorWg,
            _ => return Err(D::Error::custom("invalid feature heightmap")),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BambooConfiguration {
    pub probability: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BasaltColumnsConfiguration {
    pub height: IntProvider,
    pub reach: IntProvider,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockBlobConfiguration {
    pub state: BlockStateData,
    pub can_place_on: BlockPredicate,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockColumnConfiguration {
    #[serde(deserialize_with = "deserialize_direction")]
    pub direction: Direction,
    pub allowed_placement: BlockPredicate,
    pub layers: Vec<BlockColumnLayer>,
    pub prioritize_tip: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockColumnLayer {
    pub height: IntProvider,
    pub provider: BlockStateProvider,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockPileConfiguration {
    pub state_provider: BlockStateProvider,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeltaFeatureConfiguration {
    pub contents: BlockStateData,
    pub rim: BlockStateData,
    pub size: IntProvider,
    pub rim_size: IntProvider,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiskConfiguration {
    pub state_provider: BlockStateProvider,
    pub target: BlockPredicate,
    pub radius: IntProvider,
    pub half_height: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DripstoneClusterConfiguration {
    pub floor_to_ceiling_search_range: i32,
    pub height: IntProvider,
    pub radius: IntProvider,
    pub max_stalagmite_stalactite_height_diff: i32,
    pub height_deviation: i32,
    pub dripstone_block_layer_thickness: IntProvider,
    pub density: FloatProvider,
    pub wetness: FloatProvider,
    pub chance_of_dripstone_column_at_max_distance_from_center: f32,
    pub max_distance_from_center_affecting_height_bias: i32,
    pub max_distance_from_edge_affecting_chance_of_dripstone_column: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeleothemClusterConfiguration {
    pub base_block: BlockStateData,
    pub pointed_block: BlockStateData,
    pub replaceable_blocks: BlockHolderSet,
    pub floor_to_ceiling_search_range: i32,
    pub height: IntProvider,
    pub radius: IntProvider,
    pub max_stalagmite_stalactite_height_diff: i32,
    pub height_deviation: i32,
    pub speleothem_block_layer_thickness: IntProvider,
    pub density: FloatProvider,
    pub wetness: FloatProvider,
    pub chance_of_speleothem_at_max_distance_from_center: f32,
    pub max_distance_from_edge_affecting_chance_of_speleothem: i32,
    pub max_distance_from_center_affecting_height_bias: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeleothemConfiguration {
    pub base_block: BlockStateData,
    pub pointed_block: BlockStateData,
    pub replaceable_blocks: BlockHolderSet,
    #[serde(default = "default_speleothem_chance_of_taller_generation")]
    pub chance_of_taller_generation: f32,
    #[serde(default = "default_speleothem_chance_of_directional_spread")]
    pub chance_of_directional_spread: f32,
    #[serde(default = "default_speleothem_chance_of_spread_radius")]
    pub chance_of_spread_radius2: f32,
    #[serde(default = "default_speleothem_chance_of_spread_radius")]
    pub chance_of_spread_radius3: f32,
}

const fn default_speleothem_chance_of_taller_generation() -> f32 {
    0.2
}

const fn default_speleothem_chance_of_directional_spread() -> f32 {
    0.7
}

const fn default_speleothem_chance_of_spread_radius() -> f32 {
    0.5
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndGatewayConfiguration {
    #[serde(default)]
    pub exit: Option<Offset>,
    pub exact: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndSpikeConfiguration {
    #[serde(default)]
    pub spikes: Vec<EndSpike>,
    #[serde(default)]
    pub crystal_invulnerable: bool,
    #[serde(default)]
    pub crystal_beam_target: Option<Offset>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndSpike {
    #[serde(rename = "centerX")]
    #[serde(default)]
    pub center_x: i32,
    #[serde(rename = "centerZ")]
    #[serde(default)]
    pub center_z: i32,
    #[serde(default)]
    pub radius: i32,
    #[serde(default)]
    pub height: i32,
    #[serde(default)]
    pub guarded: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallenTreeConfiguration {
    pub trunk_provider: BlockStateProvider,
    pub log_length: IntProvider,
    pub stump_decorators: Vec<TreeDecorator>,
    pub log_decorators: Vec<TreeDecorator>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FossilConfiguration {
    pub fossil_structures: Vec<Identifier>,
    pub overlay_structures: Vec<Identifier>,
    pub fossil_processors: Identifier,
    pub overlay_processors: Identifier,
    pub max_empty_corners_allowed: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeodeConfiguration {
    pub blocks: GeodeBlockSettings,
    pub layers: GeodeLayerSettings,
    pub crack: GeodeCrackSettings,
    #[serde(default = "default_geode_use_potential_placements_chance")]
    pub use_potential_placements_chance: f64,
    #[serde(default)]
    pub use_alternate_layer0_chance: f64,
    #[serde(default = "default_true")]
    pub placements_require_layer0_alternate: bool,
    #[serde(default = "default_geode_outer_wall_distance")]
    pub outer_wall_distance: IntProvider,
    #[serde(default = "default_geode_distribution_points")]
    pub distribution_points: IntProvider,
    #[serde(default = "default_geode_point_offset")]
    pub point_offset: IntProvider,
    #[serde(default = "default_geode_min_gen_offset")]
    pub min_gen_offset: i32,
    #[serde(default = "default_geode_max_gen_offset")]
    pub max_gen_offset: i32,
    pub invalid_blocks_threshold: i32,
    #[serde(default = "default_geode_noise_multiplier")]
    pub noise_multiplier: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeodeBlockSettings {
    pub filling_provider: BlockStateProvider,
    pub inner_layer_provider: BlockStateProvider,
    pub alternate_inner_layer_provider: BlockStateProvider,
    pub middle_layer_provider: BlockStateProvider,
    pub outer_layer_provider: BlockStateProvider,
    pub inner_placements: Vec<BlockStateData>,
    #[serde(deserialize_with = "deserialize_tag_identifier")]
    pub cannot_replace: Identifier,
    #[serde(deserialize_with = "deserialize_tag_identifier")]
    pub invalid_blocks: Identifier,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeodeLayerSettings {
    #[serde(default = "default_geode_filling_layer")]
    pub filling: f64,
    #[serde(default = "default_geode_inner_layer")]
    pub inner_layer: f64,
    #[serde(default = "default_geode_middle_layer")]
    pub middle_layer: f64,
    #[serde(default = "default_geode_outer_layer")]
    pub outer_layer: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeodeCrackSettings {
    #[serde(default = "default_geode_generate_crack_chance")]
    pub generate_crack_chance: f64,
    #[serde(default = "default_geode_base_crack_size")]
    pub base_crack_size: f64,
    #[serde(default = "default_geode_crack_point_offset")]
    pub crack_point_offset: i32,
}

const fn default_true() -> bool {
    true
}

const fn default_geode_use_potential_placements_chance() -> f64 {
    0.35
}

const fn default_geode_outer_wall_distance() -> IntProvider {
    IntProvider::Uniform {
        min_inclusive: 4,
        max_inclusive: 5,
    }
}

const fn default_geode_distribution_points() -> IntProvider {
    IntProvider::Uniform {
        min_inclusive: 3,
        max_inclusive: 4,
    }
}

const fn default_geode_point_offset() -> IntProvider {
    IntProvider::Uniform {
        min_inclusive: 1,
        max_inclusive: 2,
    }
}

const fn default_geode_min_gen_offset() -> i32 {
    -16
}

const fn default_geode_max_gen_offset() -> i32 {
    16
}

const fn default_geode_noise_multiplier() -> f64 {
    0.05
}

const fn default_geode_filling_layer() -> f64 {
    1.7
}

const fn default_geode_inner_layer() -> f64 {
    2.2
}

const fn default_geode_middle_layer() -> f64 {
    3.2
}

const fn default_geode_outer_layer() -> f64 {
    4.2
}

const fn default_geode_generate_crack_chance() -> f64 {
    1.0
}

const fn default_geode_base_crack_size() -> f64 {
    2.0
}

const fn default_geode_crack_point_offset() -> i32 {
    2
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HugeMushroomConfiguration {
    pub cap_provider: BlockStateProvider,
    pub stem_provider: BlockStateProvider,
    #[serde(default = "default_huge_mushroom_foliage_radius")]
    pub foliage_radius: i32,
    pub can_place_on: BlockPredicate,
}

const fn default_huge_mushroom_foliage_radius() -> i32 {
    2
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HugeFungusConfiguration {
    pub valid_base_block: BlockStateData,
    pub stem_state: BlockStateData,
    pub hat_state: BlockStateData,
    pub decor_state: BlockStateData,
    pub replaceable_blocks: BlockPredicate,
    #[serde(default)]
    pub planted: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LakeConfiguration {
    pub fluid: BlockStateProvider,
    pub barrier: BlockStateProvider,
    pub can_place_feature: BlockPredicate,
    pub can_replace_with_air_or_fluid: BlockPredicate,
    pub can_replace_with_barrier: BlockPredicate,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LargeDripstoneConfiguration {
    pub replaceable_blocks: BlockHolderSet,
    #[serde(default = "default_large_dripstone_floor_to_ceiling_search_range")]
    pub floor_to_ceiling_search_range: i32,
    pub column_radius: IntProvider,
    pub height_scale: FloatProvider,
    pub max_column_radius_to_cave_height_ratio: f32,
    pub stalactite_bluntness: FloatProvider,
    pub stalagmite_bluntness: FloatProvider,
    pub wind_speed: FloatProvider,
    pub min_radius_for_wind: i32,
    pub min_bluntness_for_wind: f32,
}

const fn default_large_dripstone_floor_to_ceiling_search_range() -> i32 {
    30
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MultifaceGrowthConfiguration {
    pub block: Identifier,
    #[serde(default = "default_multiface_search_range")]
    pub search_range: i32,
    #[serde(default)]
    pub can_place_on_floor: bool,
    #[serde(default)]
    pub can_place_on_ceiling: bool,
    #[serde(default)]
    pub can_place_on_wall: bool,
    #[serde(default = "default_multiface_chance_of_spreading")]
    pub chance_of_spreading: f32,
    pub can_be_placed_on: Vec<Identifier>,
}

const fn default_multiface_search_range() -> i32 {
    10
}

const fn default_multiface_chance_of_spreading() -> f32 {
    0.5
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetherForestVegetationConfiguration {
    pub state_provider: BlockStateProvider,
    pub spread_width: i32,
    pub spread_height: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetherrackReplaceBlobsConfiguration {
    pub target: BlockStateData,
    pub state: BlockStateData,
    pub radius: IntProvider,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OreConfiguration {
    pub targets: Vec<OreTarget>,
    pub size: i32,
    #[serde(default)]
    pub discard_chance_on_air_exposure: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OreTarget {
    pub target: RuleTest,
    pub state: BlockStateData,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "predicate_type")]
pub enum RuleTest {
    #[serde(rename = "minecraft:block_match")]
    BlockMatch { block: Identifier },
    #[serde(rename = "minecraft:tag_match")]
    TagMatch { tag: Identifier },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointedDripstoneConfiguration {
    pub chance_of_taller_dripstone: f32,
    pub chance_of_directional_spread: f32,
    pub chance_of_spread_radius2: f32,
    pub chance_of_spread_radius3: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RandomBooleanSelectorConfiguration {
    pub feature_true: PlacedFeatureRef,
    pub feature_false: PlacedFeatureRef,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RandomSelectorConfiguration {
    pub features: Vec<WeightedPlacedFeature>,
    pub default: PlacedFeatureRef,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightedPlacedFeature {
    pub chance: f32,
    pub feature: PlacedFeatureRef,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightedRandomFeatureConfiguration {
    pub features: Vec<WeightedRandomPlacedFeature>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightedRandomPlacedFeature {
    pub data: PlacedFeatureRef,
    pub weight: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimpleRandomSelectorConfiguration {
    pub features: Vec<PlacedFeatureRef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositeFeatureConfiguration {
    #[serde(deserialize_with = "deserialize_non_empty_placed_features")]
    pub features: Vec<PlacedFeatureRef>,
}

fn deserialize_non_empty_placed_features<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<PlacedFeatureRef>, D::Error> {
    let features = Vec::<PlacedFeatureRef>::deserialize(deserializer)?;
    if features.is_empty() {
        return Err(D::Error::custom("features must not be empty"));
    }
    Ok(features)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootSystemConfiguration {
    pub feature: PlacedFeatureRef,
    pub required_vertical_space_for_tree: i32,
    pub level_test_distance: i32,
    pub max_level_deviation: i32,
    pub root_radius: i32,
    pub root_placement_attempts: i32,
    pub root_column_max_height: i32,
    pub hanging_root_radius: i32,
    pub hanging_roots_vertical_span: i32,
    pub hanging_root_placement_attempts: i32,
    pub allowed_vertical_water_for_tree: i32,
    pub root_state_provider: BlockStateProvider,
    pub hanging_root_state_provider: BlockStateProvider,
    pub root_replaceable: BlockHolderSet,
    pub allowed_tree_position: BlockPredicate,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SculkPatchConfiguration {
    pub charge_count: i32,
    pub amount_per_charge: i32,
    pub spread_attempts: i32,
    pub growth_rounds: i32,
    pub spread_rounds: i32,
    pub extra_rare_growths: IntProvider,
    pub catalyst_chance: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeaPickleConfiguration {
    pub count: IntProvider,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeagrassConfiguration {
    pub probability: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimpleBlockConfiguration {
    pub to_place: BlockStateProvider,
    #[serde(default)]
    pub schedule_tick: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpikeConfiguration {
    pub state: BlockStateData,
    pub can_place_on: BlockPredicate,
    pub can_replace: BlockPredicate,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpringConfiguration {
    pub state: FluidStateData,
    #[serde(default = "default_true")]
    pub requires_block_below: bool,
    #[serde(default = "default_spring_rock_count")]
    pub rock_count: i32,
    #[serde(default = "default_spring_hole_count")]
    pub hole_count: i32,
    pub valid_blocks: BlockHolderSet,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateFeatureConfiguration {
    pub templates: Vec<WeightedTemplateEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightedTemplateEntry {
    pub data: TemplateEntry,
    pub weight: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateEntry {
    pub id: Identifier,
    #[serde(
        default = "default_template_rotations",
        deserialize_with = "deserialize_template_rotations"
    )]
    pub rotations: Vec<Rotation>,
}

fn default_template_rotations() -> Vec<Rotation> {
    vec![
        Rotation::None,
        Rotation::Clockwise90,
        Rotation::Clockwise180,
        Rotation::CounterClockwise90,
    ]
}

fn deserialize_template_rotations<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Rotation>, D::Error> {
    let values = Vec::<String>::deserialize(deserializer)?;
    values
        .iter()
        .map(|value| parse_rotation(value).map_err(D::Error::custom))
        .collect()
}

fn parse_rotation(value: &str) -> Result<Rotation, &'static str> {
    match value {
        "none" => Ok(Rotation::None),
        "clockwise_90" => Ok(Rotation::Clockwise90),
        "180" => Ok(Rotation::Clockwise180),
        "counterclockwise_90" => Ok(Rotation::CounterClockwise90),
        _ => Err("invalid rotation"),
    }
}

const fn default_spring_rock_count() -> i32 {
    4
}

const fn default_spring_hole_count() -> i32 {
    1
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeConfiguration {
    pub trunk_provider: BlockStateProvider,
    pub below_trunk_provider: BlockStateProvider,
    pub foliage_provider: BlockStateProvider,
    pub trunk_placer: TrunkPlacer,
    pub foliage_placer: FoliagePlacer,
    pub minimum_size: FeatureSize,
    #[serde(default)]
    pub decorators: Vec<TreeDecorator>,
    #[serde(default)]
    pub root_placer: Option<RootPlacer>,
    pub ignore_vines: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum TrunkPlacer {
    #[serde(rename = "minecraft:straight_trunk_placer")]
    Straight(TrunkPlacerBase),
    #[serde(rename = "minecraft:giant_trunk_placer")]
    Giant(TrunkPlacerBase),
    #[serde(rename = "minecraft:fancy_trunk_placer")]
    Fancy(TrunkPlacerBase),
    #[serde(rename = "minecraft:forking_trunk_placer")]
    Forking(TrunkPlacerBase),
    #[serde(rename = "minecraft:dark_oak_trunk_placer")]
    DarkOak(TrunkPlacerBase),
    #[serde(rename = "minecraft:mega_jungle_trunk_placer")]
    MegaJungle(TrunkPlacerBase),
    #[serde(rename = "minecraft:bending_trunk_placer")]
    Bending(BendingTrunkPlacer),
    #[serde(rename = "minecraft:upwards_branching_trunk_placer")]
    UpwardsBranching(UpwardsBranchingTrunkPlacer),
    #[serde(rename = "minecraft:cherry_trunk_placer")]
    Cherry(CherryTrunkPlacer),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrunkPlacerBase {
    pub base_height: i32,
    pub height_rand_a: i32,
    pub height_rand_b: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BendingTrunkPlacer {
    pub base_height: i32,
    pub height_rand_a: i32,
    pub height_rand_b: i32,
    pub min_height_for_leaves: i32,
    pub bend_length: IntProvider,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpwardsBranchingTrunkPlacer {
    pub base_height: i32,
    pub height_rand_a: i32,
    pub height_rand_b: i32,
    pub extra_branch_steps: IntProvider,
    pub extra_branch_length: IntProvider,
    pub place_branch_per_log_probability: f32,
    #[serde(deserialize_with = "deserialize_tag_identifier")]
    pub can_grow_through: Identifier,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CherryTrunkPlacer {
    pub base_height: i32,
    pub height_rand_a: i32,
    pub height_rand_b: i32,
    pub branch_count: IntProvider,
    pub branch_horizontal_length: IntProvider,
    pub branch_start_offset_from_top: UniformIntProvider,
    pub branch_end_offset_from_top: IntProvider,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum FoliagePlacer {
    #[serde(rename = "minecraft:blob_foliage_placer")]
    Blob(BlobFoliagePlacer),
    #[serde(rename = "minecraft:spruce_foliage_placer")]
    Spruce(SpruceFoliagePlacer),
    #[serde(rename = "minecraft:pine_foliage_placer")]
    Pine(PineFoliagePlacer),
    #[serde(rename = "minecraft:acacia_foliage_placer")]
    Acacia(FoliagePlacerBase),
    #[serde(rename = "minecraft:bush_foliage_placer")]
    Bush(BlobFoliagePlacer),
    #[serde(rename = "minecraft:fancy_foliage_placer")]
    Fancy(BlobFoliagePlacer),
    #[serde(rename = "minecraft:jungle_foliage_placer")]
    Jungle(BlobFoliagePlacer),
    #[serde(rename = "minecraft:mega_pine_foliage_placer")]
    MegaPine(MegaPineFoliagePlacer),
    #[serde(rename = "minecraft:dark_oak_foliage_placer")]
    DarkOak(FoliagePlacerBase),
    #[serde(rename = "minecraft:random_spread_foliage_placer")]
    RandomSpread(RandomSpreadFoliagePlacer),
    #[serde(rename = "minecraft:cherry_foliage_placer")]
    Cherry(CherryFoliagePlacer),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoliagePlacerBase {
    pub radius: IntProvider,
    pub offset: IntProvider,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlobFoliagePlacer {
    pub radius: IntProvider,
    pub offset: IntProvider,
    pub height: IntProvider,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpruceFoliagePlacer {
    pub radius: IntProvider,
    pub offset: IntProvider,
    pub trunk_height: IntProvider,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PineFoliagePlacer {
    pub radius: IntProvider,
    pub offset: IntProvider,
    pub height: IntProvider,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MegaPineFoliagePlacer {
    pub radius: IntProvider,
    pub offset: IntProvider,
    pub crown_height: IntProvider,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RandomSpreadFoliagePlacer {
    pub radius: IntProvider,
    pub offset: IntProvider,
    pub foliage_height: i32,
    pub leaf_placement_attempts: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CherryFoliagePlacer {
    pub radius: IntProvider,
    pub offset: IntProvider,
    pub height: IntProvider,
    pub wide_bottom_layer_hole_chance: f32,
    pub corner_hole_chance: f32,
    pub hanging_leaves_chance: f32,
    pub hanging_leaves_extension_chance: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum FeatureSize {
    #[serde(rename = "minecraft:two_layers_feature_size")]
    TwoLayers(TwoLayersFeatureSize),
    #[serde(rename = "minecraft:three_layers_feature_size")]
    ThreeLayers(ThreeLayersFeatureSize),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TwoLayersFeatureSize {
    #[serde(default = "default_feature_size_one")]
    pub limit: i32,
    #[serde(default)]
    pub lower_size: i32,
    #[serde(default = "default_feature_size_one")]
    pub upper_size: i32,
    #[serde(default)]
    pub min_clipped_height: Option<i32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreeLayersFeatureSize {
    #[serde(default = "default_feature_size_one")]
    pub limit: i32,
    #[serde(default)]
    pub lower_size: i32,
    #[serde(default = "default_feature_size_one")]
    pub middle_size: i32,
    #[serde(default = "default_feature_size_one")]
    pub upper_limit: i32,
    #[serde(default = "default_feature_size_one")]
    pub upper_size: i32,
    #[serde(default)]
    pub min_clipped_height: Option<i32>,
}

const fn default_feature_size_one() -> i32 {
    1
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum RootPlacer {
    #[serde(rename = "minecraft:mangrove_root_placer")]
    Mangrove(MangroveRootPlacer),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MangroveRootPlacer {
    pub trunk_offset_y: IntProvider,
    pub root_provider: BlockStateProvider,
    pub above_root_placement: AboveRootPlacement,
    pub mangrove_root_placement: MangroveRootPlacement,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AboveRootPlacement {
    pub above_root_provider: BlockStateProvider,
    pub above_root_placement_chance: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MangroveRootPlacement {
    #[serde(deserialize_with = "deserialize_tag_identifier")]
    pub can_grow_through: Identifier,
    pub muddy_roots_in: Vec<Identifier>,
    pub muddy_roots_provider: BlockStateProvider,
    pub max_root_width: i32,
    pub max_root_length: i32,
    pub random_skew_chance: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum TreeDecorator {
    #[serde(rename = "minecraft:alter_ground")]
    AlterGround { provider: BlockStateProvider },
    #[serde(rename = "minecraft:beehive")]
    Beehive { probability: f32 },
    #[serde(rename = "minecraft:cocoa")]
    Cocoa { probability: f32 },
    #[serde(rename = "minecraft:creaking_heart")]
    CreakingHeart { probability: f32 },
    #[serde(rename = "minecraft:leave_vine")]
    LeaveVine { probability: f32 },
    #[serde(rename = "minecraft:trunk_vine")]
    TrunkVine,
    #[serde(rename = "minecraft:attached_to_leaves")]
    AttachedToLeaves(AttachedToLeavesDecorator),
    #[serde(rename = "minecraft:attached_to_logs")]
    AttachedToLogs(AttachedToLogsDecorator),
    #[serde(rename = "minecraft:place_on_ground")]
    PlaceOnGround(PlaceOnGroundDecorator),
    #[serde(rename = "minecraft:pale_moss")]
    PaleMoss {
        leaves_probability: f32,
        trunk_probability: f32,
        ground_probability: f32,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachedToLeavesDecorator {
    pub probability: f32,
    pub exclusion_radius_xz: i32,
    pub exclusion_radius_y: i32,
    pub required_empty_blocks: i32,
    pub block_provider: BlockStateProvider,
    #[serde(deserialize_with = "deserialize_directions")]
    pub directions: Vec<Direction>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachedToLogsDecorator {
    pub probability: f32,
    pub block_provider: BlockStateProvider,
    #[serde(deserialize_with = "deserialize_directions")]
    pub directions: Vec<Direction>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceOnGroundDecorator {
    pub block_state_provider: BlockStateProvider,
    #[serde(default = "default_place_on_ground_tries")]
    pub tries: i32,
    #[serde(default = "default_place_on_ground_radius")]
    pub radius: i32,
    #[serde(default = "default_place_on_ground_height")]
    pub height: i32,
}

const fn default_place_on_ground_tries() -> i32 {
    128
}

const fn default_place_on_ground_radius() -> i32 {
    2
}

const fn default_place_on_ground_height() -> i32 {
    1
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TwistingVinesConfiguration {
    pub spread_width: i32,
    pub spread_height: i32,
    pub max_height: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnderwaterMagmaConfiguration {
    pub floor_search_range: i32,
    pub placement_radius_around_floor: i32,
    pub placement_probability_per_valid_position: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VegetationPatchConfiguration {
    #[serde(deserialize_with = "deserialize_tag_identifier")]
    pub replaceable: Identifier,
    pub ground_state: BlockStateProvider,
    pub vegetation_feature: PlacedFeatureRef,
    pub surface: VerticalSurface,
    pub depth: IntProvider,
    pub extra_bottom_block_chance: f32,
    pub vertical_range: i32,
    pub vegetation_chance: f32,
    pub xz_radius: IntProvider,
    pub extra_edge_column_chance: f32,
}

/// Vertical surface used by vegetation patches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalSurface {
    Floor,
    Ceiling,
}

impl<'de> Deserialize<'de> for VerticalSurface {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "floor" => Self::Floor,
            "ceiling" => Self::Ceiling,
            _ => return Err(D::Error::custom("invalid vertical surface")),
        })
    }
}
