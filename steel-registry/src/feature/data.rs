//! Typed worldgen feature registry data.
//!
//! These types mirror vanilla's configured feature, placed feature, placement
//! modifier, provider, predicate, and tree-shape data after build-time decoding.
//! Runtime data stores typed registry refs where the referenced vanilla entry is
//! known at build time.

use super::{ConfiguredFeatureEntryRef, PlacedFeatureEntryRef};
use crate::blocks::BlockRef;
use crate::fluid::FluidRef;
use steel_utils::{
    Direction, Identifier,
    value_providers::{FloatProvider, HeightProvider, IntProvider, UniformIntProvider},
};

/// A configured feature reference, either a registry entry or an inline configured feature.
#[derive(Debug, Clone)]
pub enum ConfiguredFeatureRef {
    /// Registry-backed configured feature.
    Reference(ConfiguredFeatureEntryRef),
    /// Inline configured feature.
    Inline(Box<ConfiguredFeatureKind>),
}

/// A placed feature reference, either a registry entry or an inline placed feature.
#[derive(Debug, Clone)]
pub enum PlacedFeatureRef {
    /// Registry-backed placed feature.
    Reference(PlacedFeatureEntryRef),
    /// Inline placed feature.
    Inline(Box<PlacedFeatureData>),
}

/// A placed feature: configured feature plus ordered placement modifiers.
#[derive(Debug, Clone)]
pub struct PlacedFeatureData {
    /// Configured feature reference.
    pub feature: ConfiguredFeatureRef,
    /// Ordered placement modifier chain.
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
    SimpleBlock(SimpleBlockConfiguration),
    SimpleRandomSelector(SimpleRandomSelectorConfiguration),
    Spike(SpikeConfiguration),
    SpringFeature(SpringConfiguration),
    Tree(TreeConfiguration),
    TwistingVines(TwistingVinesConfiguration),
    UnderwaterMagma(UnderwaterMagmaConfiguration),
    VegetationPatch(VegetationPatchConfiguration),
    Vines,
    VoidStartPlatform,
    WaterloggedVegetationPatch(VegetationPatchConfiguration),
    WeepingVines,
}

/// Block refs decoded from vanilla's single-or-list codec shape at build time.
#[derive(Debug, Clone)]
pub struct BlockRefList(pub Vec<BlockRef>);

/// Fluid refs decoded from vanilla's single-or-list codec shape at build time.
#[derive(Debug, Clone)]
pub struct FluidRefList(pub Vec<FluidRef>);

/// Block state data emitted by the feature generator without baking a state id.
#[derive(Debug, Clone)]
pub struct BlockStateData {
    /// Referenced block entry.
    pub block: BlockRef,
    /// Explicit state properties from the extracted feature config.
    pub properties: &'static [(&'static str, &'static str)],
}

/// Fluid state data emitted by the feature generator without runtime key lookup.
#[derive(Debug, Clone)]
pub struct FluidStateData {
    /// Referenced fluid entry.
    pub fluid: FluidRef,
    /// Explicit state properties from the extracted feature config.
    pub properties: &'static [(&'static str, &'static str)],
}

/// Block position offset.
pub type Offset = [i32; 3];

/// Block predicates used by placement modifiers and feature configs.
#[derive(Debug, Clone)]
pub enum BlockPredicate {
    AllOf {
        predicates: Vec<BlockPredicate>,
    },
    AnyOf {
        predicates: Vec<BlockPredicate>,
    },
    Not {
        predicate: Box<BlockPredicate>,
    },
    MatchingBlockTag {
        tag: Identifier,
        offset: Offset,
    },
    MatchingBlocks {
        blocks: BlockRefList,
        offset: Offset,
    },
    MatchingFluids {
        fluids: FluidRefList,
        offset: Offset,
    },
    Solid {
        offset: Offset,
    },
    WouldSurvive {
        state: BlockStateData,
        offset: Offset,
    },
    Replaceable {
        offset: Offset,
    },
    HasSturdyFace {
        direction: Direction,
        offset: Offset,
    },
    InsideWorldBounds {
        offset: Offset,
    },
}

/// Block-state provider used by features.
#[derive(Debug, Clone)]
pub enum BlockStateProvider {
    Simple {
        state: BlockStateData,
    },
    Weighted {
        entries: Vec<WeightedBlockState>,
    },
    RotatedBlock {
        state: BlockStateData,
    },
    RandomizedInt {
        property: String,
        source: Box<BlockStateProvider>,
        values: IntProvider,
    },
    RuleBased {
        fallback: Option<Box<BlockStateProvider>>,
        rules: Vec<RuleBasedStateProviderRule>,
    },
    Noise(NoiseProvider),
    NoiseThreshold(NoiseThresholdProvider),
    DualNoise(DualNoiseProvider),
}

/// Weighted block-state provider entry.
#[derive(Debug, Clone)]
pub struct WeightedBlockState {
    pub data: BlockStateData,
    pub weight: i32,
}

/// Rule-based provider rule.
#[derive(Debug, Clone)]
pub struct RuleBasedStateProviderRule {
    pub if_true: BlockPredicate,
    pub then: BlockStateProvider,
}

/// Noise parameters embedded in vanilla feature providers.
#[derive(Debug, Clone)]
pub struct FeatureNoiseParameters {
    pub first_octave: i32,
    pub amplitudes: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct NoiseProvider {
    pub noise: FeatureNoiseParameters,
    pub scale: f32,
    pub seed: i64,
    pub states: Vec<BlockStateData>,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub enum PlacementModifier {
    Biome,
    BlockPredicateFilter {
        predicate: BlockPredicate,
    },
    Count {
        count: IntProvider,
    },
    CountOnEveryLayer {
        count: IntProvider,
    },
    EnvironmentScan {
        direction_of_search: Direction,
        target_condition: BlockPredicate,
        allowed_search_condition: Option<BlockPredicate>,
        max_steps: i32,
    },
    FixedPlacement {
        positions: Vec<Offset>,
    },
    HeightRange {
        height: HeightProvider,
    },
    Heightmap {
        heightmap: FeatureHeightmap,
    },
    InSquare,
    NoiseBasedCount {
        noise_to_count_ratio: i32,
        noise_factor: f64,
        noise_offset: f64,
    },
    NoiseThresholdCount {
        noise_level: f64,
        below_noise: i32,
        above_noise: i32,
    },
    RandomOffset {
        xz_spread: IntProvider,
        y_spread: IntProvider,
    },
    RarityFilter {
        chance: i32,
    },
    SurfaceRelativeThresholdFilter {
        heightmap: FeatureHeightmap,
        min_inclusive: Option<i32>,
        max_inclusive: Option<i32>,
    },
    SurfaceWaterDepthFilter {
        max_water_depth: i32,
    },
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

#[derive(Debug, Clone)]
pub struct BambooConfiguration {
    pub probability: f32,
}

#[derive(Debug, Clone)]
pub struct BasaltColumnsConfiguration {
    pub height: IntProvider,
    pub reach: IntProvider,
}

#[derive(Debug, Clone)]
pub struct BlockBlobConfiguration {
    pub state: BlockStateData,
    pub can_place_on: BlockPredicate,
}

#[derive(Debug, Clone)]
pub struct BlockColumnConfiguration {
    pub direction: Direction,
    pub allowed_placement: BlockPredicate,
    pub layers: Vec<BlockColumnLayer>,
    pub prioritize_tip: bool,
}

#[derive(Debug, Clone)]
pub struct BlockColumnLayer {
    pub height: IntProvider,
    pub provider: BlockStateProvider,
}

#[derive(Debug, Clone)]
pub struct BlockPileConfiguration {
    pub state_provider: BlockStateProvider,
}

#[derive(Debug, Clone)]
pub struct DeltaFeatureConfiguration {
    pub contents: BlockStateData,
    pub rim: BlockStateData,
    pub size: IntProvider,
    pub rim_size: IntProvider,
}

#[derive(Debug, Clone)]
pub struct DiskConfiguration {
    pub state_provider: BlockStateProvider,
    pub target: BlockPredicate,
    pub radius: IntProvider,
    pub half_height: i32,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct EndGatewayConfiguration {
    pub exit: Option<Offset>,
    pub exact: bool,
}

#[derive(Debug, Clone)]
pub struct EndSpikeConfiguration {
    pub spikes: Vec<EndSpike>,
    pub crystal_invulnerable: bool,
    pub crystal_beam_target: Option<Offset>,
}

#[derive(Debug, Clone)]
pub struct EndSpike {
    pub center_x: i32,
    pub center_z: i32,
    pub radius: i32,
    pub height: i32,
    pub guarded: bool,
}

#[derive(Debug, Clone)]
pub struct FallenTreeConfiguration {
    pub trunk_provider: BlockStateProvider,
    pub log_length: IntProvider,
    pub stump_decorators: Vec<TreeDecorator>,
    pub log_decorators: Vec<TreeDecorator>,
}

#[derive(Debug, Clone)]
pub struct FossilConfiguration {
    pub fossil_structures: Vec<Identifier>,
    pub overlay_structures: Vec<Identifier>,
    pub fossil_processors: Identifier,
    pub overlay_processors: Identifier,
    pub max_empty_corners_allowed: i32,
}

#[derive(Debug, Clone)]
pub struct GeodeConfiguration {
    pub blocks: GeodeBlockSettings,
    pub layers: GeodeLayerSettings,
    pub crack: GeodeCrackSettings,
    pub use_potential_placements_chance: f64,
    pub use_alternate_layer0_chance: f64,
    pub placements_require_layer0_alternate: bool,
    pub outer_wall_distance: IntProvider,
    pub distribution_points: IntProvider,
    pub point_offset: IntProvider,
    pub min_gen_offset: i32,
    pub max_gen_offset: i32,
    pub invalid_blocks_threshold: i32,
    pub noise_multiplier: f64,
}

#[derive(Debug, Clone)]
pub struct GeodeBlockSettings {
    pub filling_provider: BlockStateProvider,
    pub inner_layer_provider: BlockStateProvider,
    pub alternate_inner_layer_provider: BlockStateProvider,
    pub middle_layer_provider: BlockStateProvider,
    pub outer_layer_provider: BlockStateProvider,
    pub inner_placements: Vec<BlockStateData>,
    pub cannot_replace: Identifier,
    pub invalid_blocks: Identifier,
}

#[derive(Debug, Clone)]
pub struct GeodeLayerSettings {
    pub filling: f64,
    pub inner_layer: f64,
    pub middle_layer: f64,
    pub outer_layer: f64,
}

#[derive(Debug, Clone)]
pub struct GeodeCrackSettings {
    pub generate_crack_chance: f64,
    pub base_crack_size: f64,
    pub crack_point_offset: i32,
}

#[derive(Debug, Clone)]
pub struct HugeMushroomConfiguration {
    pub cap_provider: BlockStateProvider,
    pub stem_provider: BlockStateProvider,
    pub foliage_radius: i32,
    pub can_place_on: BlockPredicate,
}

#[derive(Debug, Clone)]
pub struct HugeFungusConfiguration {
    pub valid_base_block: BlockStateData,
    pub stem_state: BlockStateData,
    pub hat_state: BlockStateData,
    pub decor_state: BlockStateData,
    pub replaceable_blocks: BlockPredicate,
    pub planted: bool,
}

#[derive(Debug, Clone)]
pub struct LakeConfiguration {
    pub fluid: BlockStateProvider,
    pub barrier: BlockStateProvider,
}

#[derive(Debug, Clone)]
pub struct LargeDripstoneConfiguration {
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

#[derive(Debug, Clone)]
pub struct MultifaceGrowthConfiguration {
    pub block: BlockRef,
    pub search_range: i32,
    pub can_place_on_floor: bool,
    pub can_place_on_ceiling: bool,
    pub can_place_on_wall: bool,
    pub chance_of_spreading: f32,
    pub can_be_placed_on: Vec<BlockRef>,
}

#[derive(Debug, Clone)]
pub struct NetherForestVegetationConfiguration {
    pub state_provider: BlockStateProvider,
    pub spread_width: i32,
    pub spread_height: i32,
}

#[derive(Debug, Clone)]
pub struct NetherrackReplaceBlobsConfiguration {
    pub target: BlockStateData,
    pub state: BlockStateData,
    pub radius: IntProvider,
}

#[derive(Debug, Clone)]
pub struct OreConfiguration {
    pub targets: Vec<OreTarget>,
    pub size: i32,
    pub discard_chance_on_air_exposure: f32,
}

#[derive(Debug, Clone)]
pub struct OreTarget {
    pub target: RuleTest,
    pub state: BlockStateData,
}

#[derive(Debug, Clone)]
pub enum RuleTest {
    BlockMatch { block: BlockRef },
    TagMatch { tag: Identifier },
}

#[derive(Debug, Clone)]
pub struct PointedDripstoneConfiguration {
    pub chance_of_taller_dripstone: f32,
    pub chance_of_directional_spread: f32,
    pub chance_of_spread_radius2: f32,
    pub chance_of_spread_radius3: f32,
}

#[derive(Debug, Clone)]
pub struct RandomBooleanSelectorConfiguration {
    pub feature_true: PlacedFeatureRef,
    pub feature_false: PlacedFeatureRef,
}

#[derive(Debug, Clone)]
pub struct RandomSelectorConfiguration {
    pub features: Vec<WeightedPlacedFeature>,
    pub default: PlacedFeatureRef,
}

#[derive(Debug, Clone)]
pub struct WeightedPlacedFeature {
    pub chance: f32,
    pub feature: PlacedFeatureRef,
}

#[derive(Debug, Clone)]
pub struct SimpleRandomSelectorConfiguration {
    pub features: Vec<PlacedFeatureRef>,
}

#[derive(Debug, Clone)]
pub struct RootSystemConfiguration {
    pub feature: PlacedFeatureRef,
    pub required_vertical_space_for_tree: i32,
    pub root_radius: i32,
    pub root_placement_attempts: i32,
    pub root_column_max_height: i32,
    pub hanging_root_radius: i32,
    pub hanging_roots_vertical_span: i32,
    pub hanging_root_placement_attempts: i32,
    pub allowed_vertical_water_for_tree: i32,
    pub root_state_provider: BlockStateProvider,
    pub hanging_root_state_provider: BlockStateProvider,
    pub root_replaceable: Identifier,
    pub allowed_tree_position: BlockPredicate,
}

#[derive(Debug, Clone)]
pub struct SculkPatchConfiguration {
    pub charge_count: i32,
    pub amount_per_charge: i32,
    pub spread_attempts: i32,
    pub growth_rounds: i32,
    pub spread_rounds: i32,
    pub extra_rare_growths: IntProvider,
    pub catalyst_chance: f32,
}

#[derive(Debug, Clone)]
pub struct SeaPickleConfiguration {
    pub count: IntProvider,
}

#[derive(Debug, Clone)]
pub struct SeagrassConfiguration {
    pub probability: f32,
}

#[derive(Debug, Clone)]
pub struct SimpleBlockConfiguration {
    pub to_place: BlockStateProvider,
    pub schedule_tick: bool,
}

#[derive(Debug, Clone)]
pub struct SpikeConfiguration {
    pub state: BlockStateData,
    pub can_place_on: BlockPredicate,
    pub can_replace: BlockPredicate,
}

#[derive(Debug, Clone)]
pub struct SpringConfiguration {
    pub state: FluidStateData,
    pub requires_block_below: bool,
    pub rock_count: i32,
    pub hole_count: i32,
    pub valid_blocks: BlockRefList,
}

#[derive(Debug, Clone)]
pub struct TreeConfiguration {
    pub trunk_provider: BlockStateProvider,
    pub below_trunk_provider: BlockStateProvider,
    pub foliage_provider: BlockStateProvider,
    pub trunk_placer: TrunkPlacer,
    pub foliage_placer: FoliagePlacer,
    pub minimum_size: FeatureSize,
    pub decorators: Vec<TreeDecorator>,
    pub root_placer: Option<RootPlacer>,
    pub ignore_vines: bool,
}

#[derive(Debug, Clone)]
pub enum TrunkPlacer {
    Straight(TrunkPlacerBase),
    Giant(TrunkPlacerBase),
    Fancy(TrunkPlacerBase),
    Forking(TrunkPlacerBase),
    DarkOak(TrunkPlacerBase),
    MegaJungle(TrunkPlacerBase),
    Bending(BendingTrunkPlacer),
    UpwardsBranching(UpwardsBranchingTrunkPlacer),
    Cherry(CherryTrunkPlacer),
}

#[derive(Debug, Clone)]
pub struct TrunkPlacerBase {
    pub base_height: i32,
    pub height_rand_a: i32,
    pub height_rand_b: i32,
}

#[derive(Debug, Clone)]
pub struct BendingTrunkPlacer {
    pub base_height: i32,
    pub height_rand_a: i32,
    pub height_rand_b: i32,
    pub min_height_for_leaves: i32,
    pub bend_length: IntProvider,
}

#[derive(Debug, Clone)]
pub struct UpwardsBranchingTrunkPlacer {
    pub base_height: i32,
    pub height_rand_a: i32,
    pub height_rand_b: i32,
    pub extra_branch_steps: IntProvider,
    pub extra_branch_length: IntProvider,
    pub place_branch_per_log_probability: f32,
    pub can_grow_through: Identifier,
}

#[derive(Debug, Clone)]
pub struct CherryTrunkPlacer {
    pub base_height: i32,
    pub height_rand_a: i32,
    pub height_rand_b: i32,
    pub branch_count: IntProvider,
    pub branch_horizontal_length: IntProvider,
    pub branch_start_offset_from_top: UniformIntProvider,
    pub branch_end_offset_from_top: IntProvider,
}

#[derive(Debug, Clone)]
pub enum FoliagePlacer {
    Blob(BlobFoliagePlacer),
    Spruce(SpruceFoliagePlacer),
    Pine(PineFoliagePlacer),
    Acacia(FoliagePlacerBase),
    Bush(BlobFoliagePlacer),
    Fancy(BlobFoliagePlacer),
    Jungle(BlobFoliagePlacer),
    MegaPine(MegaPineFoliagePlacer),
    DarkOak(FoliagePlacerBase),
    RandomSpread(RandomSpreadFoliagePlacer),
    Cherry(CherryFoliagePlacer),
}

#[derive(Debug, Clone)]
pub struct FoliagePlacerBase {
    pub radius: IntProvider,
    pub offset: IntProvider,
}

#[derive(Debug, Clone)]
pub struct BlobFoliagePlacer {
    pub radius: IntProvider,
    pub offset: IntProvider,
    pub height: IntProvider,
}

#[derive(Debug, Clone)]
pub struct SpruceFoliagePlacer {
    pub radius: IntProvider,
    pub offset: IntProvider,
    pub trunk_height: IntProvider,
}

#[derive(Debug, Clone)]
pub struct PineFoliagePlacer {
    pub radius: IntProvider,
    pub offset: IntProvider,
    pub height: IntProvider,
}

#[derive(Debug, Clone)]
pub struct MegaPineFoliagePlacer {
    pub radius: IntProvider,
    pub offset: IntProvider,
    pub crown_height: IntProvider,
}

#[derive(Debug, Clone)]
pub struct RandomSpreadFoliagePlacer {
    pub radius: IntProvider,
    pub offset: IntProvider,
    pub foliage_height: i32,
    pub leaf_placement_attempts: i32,
}

#[derive(Debug, Clone)]
pub struct CherryFoliagePlacer {
    pub radius: IntProvider,
    pub offset: IntProvider,
    pub height: IntProvider,
    pub wide_bottom_layer_hole_chance: f32,
    pub corner_hole_chance: f32,
    pub hanging_leaves_chance: f32,
    pub hanging_leaves_extension_chance: f32,
}

#[derive(Debug, Clone)]
pub enum FeatureSize {
    TwoLayers(TwoLayersFeatureSize),
    ThreeLayers(ThreeLayersFeatureSize),
}

#[derive(Debug, Clone)]
pub struct TwoLayersFeatureSize {
    pub limit: i32,
    pub lower_size: i32,
    pub upper_size: i32,
    pub min_clipped_height: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct ThreeLayersFeatureSize {
    pub limit: i32,
    pub lower_size: i32,
    pub middle_size: i32,
    pub upper_limit: i32,
    pub upper_size: i32,
    pub min_clipped_height: Option<i32>,
}

#[derive(Debug, Clone)]
pub enum RootPlacer {
    Mangrove(MangroveRootPlacer),
}

#[derive(Debug, Clone)]
pub struct MangroveRootPlacer {
    pub trunk_offset_y: IntProvider,
    pub root_provider: BlockStateProvider,
    pub above_root_placement: AboveRootPlacement,
    pub mangrove_root_placement: MangroveRootPlacement,
}

#[derive(Debug, Clone)]
pub struct AboveRootPlacement {
    pub above_root_provider: BlockStateProvider,
    pub above_root_placement_chance: f32,
}

#[derive(Debug, Clone)]
pub struct MangroveRootPlacement {
    pub can_grow_through: Identifier,
    pub muddy_roots_in: Vec<Identifier>,
    pub muddy_roots_provider: BlockStateProvider,
    pub max_root_width: i32,
    pub max_root_length: i32,
    pub random_skew_chance: f32,
}

#[derive(Debug, Clone)]
pub enum TreeDecorator {
    AlterGround {
        provider: BlockStateProvider,
    },
    Beehive {
        probability: f32,
    },
    Cocoa {
        probability: f32,
    },
    CreakingHeart {
        probability: f32,
    },
    LeaveVine {
        probability: f32,
    },
    TrunkVine,
    AttachedToLeaves(AttachedToLeavesDecorator),
    AttachedToLogs(AttachedToLogsDecorator),
    PlaceOnGround(PlaceOnGroundDecorator),
    PaleMoss {
        leaves_probability: f32,
        trunk_probability: f32,
        ground_probability: f32,
    },
}

#[derive(Debug, Clone)]
pub struct AttachedToLeavesDecorator {
    pub probability: f32,
    pub exclusion_radius_xz: i32,
    pub exclusion_radius_y: i32,
    pub required_empty_blocks: i32,
    pub block_provider: BlockStateProvider,
    pub directions: Vec<Direction>,
}

#[derive(Debug, Clone)]
pub struct AttachedToLogsDecorator {
    pub probability: f32,
    pub block_provider: BlockStateProvider,
    pub directions: Vec<Direction>,
}

#[derive(Debug, Clone)]
pub struct PlaceOnGroundDecorator {
    pub block_state_provider: BlockStateProvider,
    pub tries: i32,
    pub radius: i32,
    pub height: i32,
}

#[derive(Debug, Clone)]
pub struct TwistingVinesConfiguration {
    pub spread_width: i32,
    pub spread_height: i32,
    pub max_height: i32,
}

#[derive(Debug, Clone)]
pub struct UnderwaterMagmaConfiguration {
    pub floor_search_range: i32,
    pub placement_radius_around_floor: i32,
    pub placement_probability_per_valid_position: f32,
}

#[derive(Debug, Clone)]
pub struct VegetationPatchConfiguration {
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
