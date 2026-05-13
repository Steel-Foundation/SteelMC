//! Biome decoration runner for the `FEATURES` chunk stage.
//!
//! Vanilla treats biome decoration as one ordered pass over structure pieces and placed
//! features. This module builds the same per-step placed-feature ordering up front and
//! drives the per-chunk decoration seed loop. Placed-feature modifiers and selector
//! configured features execute normally; concrete block-mutating configured features are
//! added as their foundations are implemented.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use rustc_hash::{FxHashMap, FxHashSet};
use steel_registry::biome::BiomeRef;
use steel_registry::blocks::{
    BlockRef, block_state_ext::BlockStateExt as _, properties::BlockStateProperties,
    properties::DoubleBlockHalf, properties::EnumProperty, properties::WallSide, shapes,
};
use steel_registry::feature::{
    BlockBlobConfiguration, BlockColumnConfiguration, BlockPileConfiguration, BlockPredicate,
    BlockStateData, BlockStateProvider, ConfiguredFeatureKind, ConfiguredFeatureRef,
    DiskConfiguration, DualNoiseProvider, FeatureHeightmap, FeatureNoiseParameters, NoiseProvider,
    NoiseThresholdProvider, PlacedFeatureData, PlacedFeatureEntryRef, PlacedFeatureRef,
    PlacementModifier, SimpleBlockConfiguration,
};
use steel_registry::fluid::FluidStateExt as _;
use steel_registry::{
    Registry, RegistryEntry as _, RegistryExt as _, TaggedRegistryExt as _, vanilla_blocks,
};
use steel_utils::math::Axis;
use steel_utils::random::{
    Random as _, RandomSource, legacy_random::LegacyRandom, xoroshiro::Xoroshiro,
};
use steel_utils::types::UpdateFlags;
use steel_utils::value_providers::IntProvider;
use steel_utils::{BlockPos, BlockStateId, Direction, SectionPos};
use steel_worldgen::noise::{NormalNoise, PerlinSimplexNoise};

use crate::chunk::chunk_access::ChunkStatus;
use crate::chunk::heightmap::HeightmapType;
use crate::fluid::state::get_fluid_state_from_block;
use crate::worldgen::region::WorldGenRegion;
use crate::{behavior::BLOCK_BEHAVIORS, worldgen::generators::vanilla::fuzzed_biome_at_block};

const DECORATION_STEP_COUNT: usize = 11;

static BIOME_INFO_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = RandomSource::Legacy(LegacyRandom::from_seed(2345));
    PerlinSimplexNoise::new(&mut random, &[0])
});

/// Cached vanilla ordering for all placed features reachable from a biome source.
#[derive(Debug)]
pub(crate) struct FeatureSorter {
    steps: Box<[FeatureStepData]>,
}

#[derive(Debug)]
struct FeatureStepData {
    features: Box<[PlacedFeatureEntryRef]>,
    index_by_placed_feature_id: FxHashMap<usize, usize>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct FeatureVertex {
    step: usize,
    order: usize,
    placed_feature_id: usize,
}

impl Ord for FeatureVertex {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.step, self.order, self.placed_feature_id).cmp(&(
            other.step,
            other.order,
            other.placed_feature_id,
        ))
    }
}

impl PartialOrd for FeatureVertex {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl FeatureSorter {
    #[must_use]
    fn build(possible_biomes: &[BiomeRef], registry: &Registry) -> Self {
        let mut feature_order_by_id = FxHashMap::default();
        let mut next_feature_order = 0usize;
        let mut edges = BTreeMap::<FeatureVertex, BTreeSet<FeatureVertex>>::new();

        for biome in possible_biomes {
            let mut biome_features = Vec::new();

            for (step, feature_stage) in biome.features.iter().enumerate() {
                for feature_key in feature_stage {
                    let Some(placed_feature_id) = registry.placed_features.id_from_key(feature_key)
                    else {
                        panic!(
                            "biome {} references unknown placed feature {}",
                            biome.key, feature_key
                        );
                    };

                    let feature_order =
                        if let Some(&order) = feature_order_by_id.get(&placed_feature_id) {
                            order
                        } else {
                            let order = next_feature_order;
                            next_feature_order += 1;
                            feature_order_by_id.insert(placed_feature_id, order);
                            order
                        };

                    let vertex = FeatureVertex {
                        step,
                        order: feature_order,
                        placed_feature_id,
                    };
                    edges.entry(vertex).or_default();
                    biome_features.push(vertex);
                }
            }

            for feature_pair in biome_features.windows(2) {
                edges
                    .entry(feature_pair[0])
                    .or_default()
                    .insert(feature_pair[1]);
            }
        }

        let sorted_features = Self::topological_sort(&edges);
        Self::from_sorted_features(&sorted_features, registry)
    }

    #[must_use]
    fn step_count(&self) -> usize {
        self.steps.len()
    }

    fn step(&self, step: usize) -> Option<&FeatureStepData> {
        self.steps.get(step)
    }

    fn topological_sort(
        edges: &BTreeMap<FeatureVertex, BTreeSet<FeatureVertex>>,
    ) -> Vec<FeatureVertex> {
        let mut sorted = Vec::with_capacity(edges.len());
        let mut discovered = BTreeSet::new();
        let mut visiting = BTreeSet::new();
        let vertices = edges.keys().copied().collect::<Vec<_>>();

        for vertex in vertices {
            assert!(
                !Self::visit(vertex, edges, &mut discovered, &mut visiting, &mut sorted),
                "biome decoration placed-feature order contains a cycle"
            );
        }

        sorted.reverse();
        sorted
    }

    fn visit(
        vertex: FeatureVertex,
        edges: &BTreeMap<FeatureVertex, BTreeSet<FeatureVertex>>,
        discovered: &mut BTreeSet<FeatureVertex>,
        visiting: &mut BTreeSet<FeatureVertex>,
        sorted: &mut Vec<FeatureVertex>,
    ) -> bool {
        if discovered.contains(&vertex) {
            return false;
        }
        if !visiting.insert(vertex) {
            return true;
        }

        if let Some(neighbors) = edges.get(&vertex) {
            for &neighbor in neighbors {
                if Self::visit(neighbor, edges, discovered, visiting, sorted) {
                    return true;
                }
            }
        }

        visiting.remove(&vertex);
        discovered.insert(vertex);
        sorted.push(vertex);
        false
    }

    #[must_use]
    fn from_sorted_features(sorted_features: &[FeatureVertex], registry: &Registry) -> Self {
        let Some(max_step) = sorted_features.iter().map(|feature| feature.step).max() else {
            return Self {
                steps: Box::new([]),
            };
        };

        let mut steps = Vec::with_capacity(max_step + 1);
        for step in 0..=max_step {
            let mut features = Vec::new();
            let mut index_by_placed_feature_id = FxHashMap::default();

            for feature in sorted_features
                .iter()
                .filter(|feature| feature.step == step)
            {
                let Some(placed_feature) =
                    registry.placed_features.by_id(feature.placed_feature_id)
                else {
                    panic!(
                        "feature sorter references unknown placed feature id {}",
                        feature.placed_feature_id
                    );
                };
                let index = features.len();
                features.push(placed_feature);
                index_by_placed_feature_id.insert(feature.placed_feature_id, index);
            }

            steps.push(FeatureStepData {
                features: features.into_boxed_slice(),
                index_by_placed_feature_id,
            });
        }

        Self {
            steps: steps.into_boxed_slice(),
        }
    }
}

impl FeatureStepData {
    fn feature_index(&self, placed_feature_id: usize) -> Option<usize> {
        self.index_by_placed_feature_id
            .get(&placed_feature_id)
            .copied()
    }

    fn feature(&self, index: usize) -> Option<PlacedFeatureEntryRef> {
        self.features.get(index).copied()
    }
}

/// Runs the structure-piece and placed-feature decoration pass for a generator.
#[derive(Debug)]
pub(crate) struct FeatureDecorationRunner {
    sorter: FeatureSorter,
    source_biome_ids: FxHashSet<usize>,
}

impl FeatureDecorationRunner {
    #[must_use]
    pub(crate) fn new(possible_biomes: &[BiomeRef], registry: &Registry) -> Self {
        let mut source_biome_ids = FxHashSet::default();
        let mut unique_biomes = Vec::new();

        for &biome in possible_biomes {
            let Some(biome_id) = biome.try_id() else {
                panic!("possible biome {} is not registered", biome.key);
            };

            if source_biome_ids.insert(biome_id) {
                unique_biomes.push(biome);
            }
        }

        Self {
            sorter: FeatureSorter::build(&unique_biomes, registry),
            source_biome_ids,
        }
    }

    pub(crate) fn decorate(
        &self,
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        seed: i64,
        biome_zoom_seed: i64,
    ) {
        let center = region.center();
        let origin = BlockPos::new(center.0.x * 16, region.min_y(), center.0.y * 16);
        let possible_biomes = self.collect_possible_biome_ids(region);

        let mut random = Xoroshiro::from_seed(0);
        let decoration_seed = random.set_decoration_seed(seed, origin.x(), origin.z());
        let step_count = DECORATION_STEP_COUNT.max(self.sorter.step_count());

        for step in 0..step_count {
            Self::place_structures_for_step(region, step);

            let Some(step_features) = self.sorter.step(step) else {
                continue;
            };
            Self::place_features_for_step(
                region,
                registry,
                decoration_seed,
                &mut random,
                origin,
                step,
                step_features,
                &possible_biomes,
                biome_zoom_seed,
            );
        }
    }

    fn collect_possible_biome_ids(&self, region: &WorldGenRegion<'_>) -> Vec<usize> {
        let center = region.center();
        let mut biomes = FxHashSet::default();

        for chunk_z in center.0.y - 1..=center.0.y + 1 {
            for chunk_x in center.0.x - 1..=center.0.x + 1 {
                let chunk = region.chunk(chunk_x, chunk_z, ChunkStatus::Biomes);
                for biome_id in chunk.sections().read_all_biomes() {
                    let biome_id = usize::from(biome_id);
                    if self.source_biome_ids.contains(&biome_id) {
                        biomes.insert(biome_id);
                    }
                }
            }
        }

        let mut biomes = biomes.into_iter().collect::<Vec<_>>();
        biomes.sort_unstable();
        biomes
    }

    const fn place_structures_for_step(_region: &mut WorldGenRegion<'_>, _step: usize) {
        // TODO: Place generated structure pieces once template block payloads are extracted.
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors vanilla's decoration loop state without hiding generation inputs"
    )]
    fn place_features_for_step(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        decoration_seed: i64,
        random: &mut Xoroshiro,
        origin: BlockPos,
        step: usize,
        step_features: &FeatureStepData,
        possible_biomes: &[usize],
        biome_zoom_seed: i64,
    ) {
        let mut feature_indices = FxHashSet::default();

        for &biome_id in possible_biomes {
            let Some(biome) = registry.biomes.by_id(biome_id) else {
                panic!("chunk biome palette references unknown biome id {biome_id}");
            };
            let Some(feature_stage) = biome.features.get(step) else {
                continue;
            };

            for feature_key in feature_stage {
                let Some(placed_feature_id) = registry.placed_features.id_from_key(feature_key)
                else {
                    panic!(
                        "biome {} references unknown placed feature {}",
                        biome.key, feature_key
                    );
                };
                let Some(feature_index) = step_features.feature_index(placed_feature_id) else {
                    panic!(
                        "placed feature {} from biome {} was not included in decoration step {}",
                        feature_key, biome.key, step
                    );
                };
                feature_indices.insert(feature_index);
            }
        }

        let mut feature_indices = feature_indices.into_iter().collect::<Vec<_>>();
        feature_indices.sort_unstable();

        for feature_index in feature_indices {
            let Ok(feature_index_i32) = i32::try_from(feature_index) else {
                panic!("decoration feature index {feature_index} exceeds i32 range");
            };
            let Ok(step_i32) = i32::try_from(step) else {
                panic!("decoration step {step} exceeds i32 range");
            };
            random.set_feature_seed(decoration_seed, feature_index_i32, step_i32);
            let Some(feature) = step_features.feature(feature_index) else {
                panic!("decoration step {step} references missing feature index {feature_index}");
            };
            Self::place_placed_feature_entry(
                region,
                registry,
                random,
                origin,
                feature,
                biome_zoom_seed,
            );
        }
    }

    fn place_placed_feature_entry(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        origin: BlockPos,
        feature: PlacedFeatureEntryRef,
        biome_zoom_seed: i64,
    ) -> bool {
        let Some(feature_id) = feature.try_id() else {
            panic!("top-level placed feature {} is not registered", feature.key);
        };
        Self::place_placed_feature_data(
            region,
            registry,
            random,
            origin,
            &feature.data,
            Some(feature_id),
            biome_zoom_seed,
        )
    }

    fn place_placed_feature_data(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        origin: BlockPos,
        feature: &PlacedFeatureData,
        biome_filter_feature_id: Option<usize>,
        biome_zoom_seed: i64,
    ) -> bool {
        let mut positions = vec![origin];

        for modifier in &feature.placement {
            let mut next_positions = Vec::new();
            for position in positions {
                next_positions.extend(Self::apply_placement_modifier(
                    region,
                    registry,
                    random,
                    position,
                    biome_filter_feature_id,
                    modifier,
                    biome_zoom_seed,
                ));
            }

            if next_positions.is_empty() {
                return false;
            }
            positions = next_positions;
        }

        let mut placed = false;
        for position in positions {
            placed |= Self::place_configured_feature(
                region,
                registry,
                random,
                &feature.feature,
                position,
                biome_zoom_seed,
            );
        }
        placed
    }

    fn place_placed_feature_ref(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        origin: BlockPos,
        feature: &PlacedFeatureRef,
        biome_zoom_seed: i64,
    ) -> bool {
        let feature_data = match feature {
            PlacedFeatureRef::Reference(key) => {
                let Some(feature) = registry.placed_features.by_key(key) else {
                    panic!("configured selector references unknown placed feature {key}");
                };
                &feature.data
            }
            PlacedFeatureRef::Inline(feature) => feature,
        };

        Self::place_placed_feature_data(
            region,
            registry,
            random,
            origin,
            feature_data,
            None,
            biome_zoom_seed,
        )
    }

    #[expect(
        clippy::too_many_lines,
        reason = "keeps the vanilla placement modifier dispatch table in one place"
    )]
    fn apply_placement_modifier(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        origin: BlockPos,
        biome_filter_feature_id: Option<usize>,
        modifier: &PlacementModifier,
        biome_zoom_seed: i64,
    ) -> Vec<BlockPos> {
        match modifier {
            PlacementModifier::Biome => {
                if Self::biome_allows_feature(
                    region,
                    registry,
                    biome_zoom_seed,
                    origin,
                    biome_filter_feature_id,
                ) {
                    vec![origin]
                } else {
                    Vec::new()
                }
            }
            PlacementModifier::BlockPredicateFilter { predicate } => {
                if Self::test_block_predicate(region, registry, predicate, origin) {
                    vec![origin]
                } else {
                    Vec::new()
                }
            }
            PlacementModifier::Count { count } => {
                Self::repeat_position(origin, count.sample(random))
            }
            PlacementModifier::CountOnEveryLayer { count } => {
                Self::count_on_every_layer_positions(region, random, origin, count)
            }
            PlacementModifier::EnvironmentScan {
                direction_of_search,
                target_condition,
                allowed_search_condition,
                max_steps,
            } => Self::environment_scan_positions(
                region,
                registry,
                origin,
                *direction_of_search,
                target_condition,
                allowed_search_condition.as_ref(),
                *max_steps,
            ),
            PlacementModifier::FixedPlacement { positions } => {
                let chunk_x = SectionPos::block_to_section_coord(origin.x());
                let chunk_z = SectionPos::block_to_section_coord(origin.z());
                positions
                    .iter()
                    .map(|position| BlockPos::new(position[0], position[1], position[2]))
                    .filter(|position| {
                        chunk_x == SectionPos::block_to_section_coord(position.x())
                            && chunk_z == SectionPos::block_to_section_coord(position.z())
                    })
                    .collect()
            }
            PlacementModifier::HeightRange { height } => vec![BlockPos::new(
                origin.x(),
                height.sample(random, region.min_y(), region.height()),
                origin.z(),
            )],
            PlacementModifier::Heightmap { heightmap } => {
                let height = region.height_at(
                    Self::feature_heightmap_type(*heightmap),
                    origin.x(),
                    origin.z(),
                );
                if height > region.min_y() {
                    vec![BlockPos::new(origin.x(), height, origin.z())]
                } else {
                    Vec::new()
                }
            }
            PlacementModifier::InSquare => vec![BlockPos::new(
                origin.x() + random.next_i32_bounded(16),
                origin.y(),
                origin.z() + random.next_i32_bounded(16),
            )],
            PlacementModifier::NoiseBasedCount {
                noise_to_count_ratio,
                noise_factor,
                noise_offset,
            } => {
                let noise = BIOME_INFO_NOISE.get_value(
                    f64::from(origin.x()) / *noise_factor,
                    f64::from(origin.z()) / *noise_factor,
                );
                let count =
                    ((noise + *noise_offset) * f64::from(*noise_to_count_ratio)).ceil() as i32;
                Self::repeat_position(origin, count)
            }
            PlacementModifier::NoiseThresholdCount {
                noise_level,
                below_noise,
                above_noise,
            } => {
                let noise = BIOME_INFO_NOISE
                    .get_value(f64::from(origin.x()) / 200.0, f64::from(origin.z()) / 200.0);
                let count = if noise < *noise_level {
                    *below_noise
                } else {
                    *above_noise
                };
                Self::repeat_position(origin, count)
            }
            PlacementModifier::RandomOffset {
                xz_spread,
                y_spread,
            } => vec![BlockPos::new(
                origin.x() + xz_spread.sample(random),
                origin.y() + y_spread.sample(random),
                origin.z() + xz_spread.sample(random),
            )],
            PlacementModifier::RarityFilter { chance } => {
                assert!(
                    *chance > 0,
                    "rarity filter chance must be positive, got {chance}"
                );
                if random.next_f32() < 1.0 / (*chance as f32) {
                    vec![origin]
                } else {
                    Vec::new()
                }
            }
            PlacementModifier::SurfaceRelativeThresholdFilter {
                heightmap,
                min_inclusive,
                max_inclusive,
            } => {
                let surface_y = i64::from(region.height_at(
                    Self::feature_heightmap_type(*heightmap),
                    origin.x(),
                    origin.z(),
                ));
                let min_y = surface_y + i64::from(min_inclusive.unwrap_or(i32::MIN));
                let max_y = surface_y + i64::from(max_inclusive.unwrap_or(i32::MAX));
                let origin_y = i64::from(origin.y());
                if min_y <= origin_y && origin_y <= max_y {
                    vec![origin]
                } else {
                    Vec::new()
                }
            }
            PlacementModifier::SurfaceWaterDepthFilter { max_water_depth } => {
                let ocean_floor =
                    region.height_at(HeightmapType::OceanFloor, origin.x(), origin.z());
                let surface = region.height_at(HeightmapType::WorldSurface, origin.x(), origin.z());
                if surface - ocean_floor <= *max_water_depth {
                    vec![origin]
                } else {
                    Vec::new()
                }
            }
        }
    }

    fn place_configured_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        feature: &ConfiguredFeatureRef,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) -> bool {
        match Self::configured_feature_kind(registry, feature) {
            ConfiguredFeatureKind::RandomBooleanSelector(config) => {
                let selected_feature = if random.next_bool() {
                    &config.feature_true
                } else {
                    &config.feature_false
                };
                Self::place_placed_feature_ref(
                    region,
                    registry,
                    random,
                    origin,
                    selected_feature,
                    biome_zoom_seed,
                )
            }
            ConfiguredFeatureKind::RandomSelector(config) => {
                for weighted_feature in &config.features {
                    if random.next_f32() < weighted_feature.chance {
                        return Self::place_placed_feature_ref(
                            region,
                            registry,
                            random,
                            origin,
                            &weighted_feature.feature,
                            biome_zoom_seed,
                        );
                    }
                }

                Self::place_placed_feature_ref(
                    region,
                    registry,
                    random,
                    origin,
                    &config.default,
                    biome_zoom_seed,
                )
            }
            ConfiguredFeatureKind::SimpleRandomSelector(config) => {
                assert!(
                    !config.features.is_empty(),
                    "simple random selector feature list must not be empty"
                );
                let Ok(feature_count) = i32::try_from(config.features.len()) else {
                    panic!(
                        "simple random selector feature count {} exceeds i32 range",
                        config.features.len()
                    );
                };
                let feature_index = random.next_i32_bounded(feature_count) as usize;
                Self::place_placed_feature_ref(
                    region,
                    registry,
                    random,
                    origin,
                    &config.features[feature_index],
                    biome_zoom_seed,
                )
            }
            ConfiguredFeatureKind::SimpleBlock(config) => {
                Self::place_simple_block_feature(region, registry, random, config, origin)
            }
            ConfiguredFeatureKind::BlockBlob(config) => {
                Self::place_block_blob_feature(region, registry, random, config, origin)
            }
            ConfiguredFeatureKind::BlockColumn(config) => {
                Self::place_block_column_feature(region, registry, random, config, origin)
            }
            ConfiguredFeatureKind::BlockPile(config) => {
                Self::place_block_pile_feature(region, registry, random, config, origin)
            }
            ConfiguredFeatureKind::Disk(config) => {
                Self::place_disk_feature(region, registry, random, config, origin)
            }
            ConfiguredFeatureKind::BasaltPillar => {
                Self::place_basalt_pillar_feature(region, random, origin)
            }
            _ => {
                // TODO: Dispatch concrete block-mutating feature implementations as they are added.
                false
            }
        }
    }

    fn place_block_blob_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &BlockBlobConfiguration,
        mut origin: BlockPos,
    ) -> bool {
        while origin.y() > region.min_y() + 3
            && !Self::test_block_predicate(region, registry, &config.can_place_on, origin.below())
        {
            origin = origin.below();
        }

        if origin.y() <= region.min_y() + 3 {
            return false;
        }

        let state = Self::block_state_from_data(registry, &config.state);
        for _ in 0..3 {
            let x_radius = random.next_i32_bounded(2);
            let y_radius = random.next_i32_bounded(2);
            let z_radius = random.next_i32_bounded(2);
            let threshold = (x_radius + y_radius + z_radius) as f32 * 0.333 + 0.5;
            let threshold_squared = threshold * threshold;

            for x in origin.x() - x_radius..=origin.x() + x_radius {
                for y in origin.y() - y_radius..=origin.y() + y_radius {
                    for z in origin.z() - z_radius..=origin.z() + z_radius {
                        let dx = x - origin.x();
                        let dy = y - origin.y();
                        let dz = z - origin.z();
                        if (dx * dx + dy * dy + dz * dz) as f32 <= threshold_squared {
                            let _ = region.set_block_state(
                                BlockPos::new(x, y, z),
                                state,
                                UpdateFlags::UPDATE_ALL,
                            );
                        }
                    }
                }
            }

            origin = origin.offset(
                -1 + random.next_i32_bounded(2),
                -random.next_i32_bounded(2),
                -1 + random.next_i32_bounded(2),
            );
        }

        true
    }

    fn place_block_column_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &BlockColumnConfiguration,
        origin: BlockPos,
    ) -> bool {
        let mut layer_heights = config
            .layers
            .iter()
            .map(|layer| layer.height.sample(random))
            .collect::<Vec<_>>();
        let total_height = layer_heights.iter().sum::<i32>();
        if total_height == 0 {
            return false;
        }

        let mut next_pos = origin.relative(config.direction);
        for height in 0..total_height {
            if !Self::test_block_predicate(region, registry, &config.allowed_placement, next_pos) {
                Self::truncate_block_column_layers(
                    &mut layer_heights,
                    total_height,
                    height,
                    config.prioritize_tip,
                );
                break;
            }
            next_pos = next_pos.relative(config.direction);
        }

        let mut place_pos = origin;
        for (layer_index, layer) in config.layers.iter().enumerate() {
            for _ in 0..layer_heights[layer_index] {
                let state = Self::sample_block_state_provider(
                    region,
                    registry,
                    random,
                    &layer.provider,
                    place_pos,
                );
                let _ = region.set_block_state(place_pos, state, UpdateFlags::UPDATE_CLIENTS);
                place_pos = place_pos.relative(config.direction);
            }
        }

        true
    }

    fn truncate_block_column_layers(
        layer_heights: &mut [i32],
        total_height: i32,
        new_height: i32,
        prioritize_tip: bool,
    ) {
        let mut amount_to_remove = total_height - new_height;
        if prioritize_tip {
            for height in layer_heights {
                if amount_to_remove == 0 {
                    return;
                }
                let removed = (*height).min(amount_to_remove);
                amount_to_remove -= removed;
                *height -= removed;
            }
        } else {
            for height in layer_heights.iter_mut().rev() {
                if amount_to_remove == 0 {
                    return;
                }
                let removed = (*height).min(amount_to_remove);
                amount_to_remove -= removed;
                *height -= removed;
            }
        }
    }

    fn place_block_pile_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &BlockPileConfiguration,
        origin: BlockPos,
    ) -> bool {
        if origin.y() < region.min_y() + 5 {
            return false;
        }

        let x_radius = 2 + random.next_i32_bounded(2);
        let z_radius = 2 + random.next_i32_bounded(2);

        for x in origin.x() - x_radius..=origin.x() + x_radius {
            for y in origin.y()..=origin.y() + 1 {
                for z in origin.z() - z_radius..=origin.z() + z_radius {
                    let dx = origin.x() - x;
                    let dz = origin.z() - z;
                    let distance_squared = (dx * dx + dz * dz) as f32;
                    if distance_squared <= random.next_f32() * 10.0 - random.next_f32() * 6.0
                        || random.next_f32() < 0.031
                    {
                        Self::try_place_block_pile_block(
                            region,
                            registry,
                            random,
                            config,
                            BlockPos::new(x, y, z),
                        );
                    }
                }
            }
        }

        true
    }

    fn try_place_block_pile_block(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &BlockPileConfiguration,
        pos: BlockPos,
    ) {
        if !region.block_state(pos).is_air() || !Self::block_pile_may_place_on(region, random, pos)
        {
            return;
        }

        let state = Self::sample_block_state_provider(
            region,
            registry,
            random,
            &config.state_provider,
            pos,
        );
        let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_NONE);
    }

    fn block_pile_may_place_on(
        region: &WorldGenRegion<'_>,
        random: &mut Xoroshiro,
        pos: BlockPos,
    ) -> bool {
        let below = region.block_state(pos.below());
        if below.get_block() == &vanilla_blocks::DIRT_PATH {
            return random.next_bool();
        }

        below.is_face_sturdy(Direction::Up)
    }

    fn place_disk_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &DiskConfiguration,
        origin: BlockPos,
    ) -> bool {
        let top = origin.y() + config.half_height;
        let bottom = origin.y() - config.half_height - 1;
        let radius = config.radius.sample(random);
        let mut placed_any = false;

        for x in origin.x() - radius..=origin.x() + radius {
            for z in origin.z() - radius..=origin.z() + radius {
                let dx = x - origin.x();
                let dz = z - origin.z();
                if dx * dx + dz * dz <= radius * radius {
                    placed_any |= Self::place_disk_column(
                        region,
                        registry,
                        random,
                        config,
                        top,
                        bottom,
                        BlockPos::new(x, origin.y(), z),
                    );
                }
            }
        }

        placed_any
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "matches vanilla disk column placement state"
    )]
    fn place_disk_column(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &DiskConfiguration,
        top: i32,
        bottom: i32,
        column_pos: BlockPos,
    ) -> bool {
        let mut placed_any = false;
        let mut placed_above = false;

        for y in (bottom + 1..=top).rev() {
            let pos = BlockPos::new(column_pos.x(), y, column_pos.z());
            if Self::test_block_predicate(region, registry, &config.target, pos) {
                if let Some(state) = Self::sample_block_state_provider_optional(
                    region,
                    registry,
                    random,
                    &config.state_provider,
                    pos,
                ) {
                    let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS);
                    if !placed_above {
                        Self::mark_above_for_postprocessing(region, pos);
                    }
                    placed_any = true;
                    placed_above = true;
                }
            } else {
                placed_above = false;
            }
        }

        placed_any
    }

    fn mark_above_for_postprocessing(region: &WorldGenRegion<'_>, pos: BlockPos) {
        let mut mark_pos = pos;
        for _ in 0..2 {
            mark_pos = mark_pos.above();
            if region.block_state(mark_pos).is_air() {
                return;
            }
            region.mark_pos_for_postprocessing(mark_pos);
        }
    }

    fn place_basalt_pillar_feature(
        region: &mut WorldGenRegion<'_>,
        random: &mut Xoroshiro,
        origin: BlockPos,
    ) -> bool {
        if !region.block_state(origin).is_air() || region.block_state(origin.above()).is_air() {
            return false;
        }

        let basalt = vanilla_blocks::BASALT.default_state();
        let mut pos = origin;
        let mut place_north_hangoff = true;
        let mut place_south_hangoff = true;
        let mut place_west_hangoff = true;
        let mut place_east_hangoff = true;

        while region.block_state(pos).is_air() {
            if region.is_outside_build_height(pos.y()) {
                return true;
            }

            let _ = region.set_block_state(pos, basalt, UpdateFlags::UPDATE_CLIENTS);
            if place_north_hangoff {
                place_north_hangoff =
                    Self::place_basalt_pillar_hangoff(region, random, basalt, pos.north());
            }
            if place_south_hangoff {
                place_south_hangoff =
                    Self::place_basalt_pillar_hangoff(region, random, basalt, pos.south());
            }
            if place_west_hangoff {
                place_west_hangoff =
                    Self::place_basalt_pillar_hangoff(region, random, basalt, pos.west());
            }
            if place_east_hangoff {
                place_east_hangoff =
                    Self::place_basalt_pillar_hangoff(region, random, basalt, pos.east());
            }

            pos = pos.below();
        }

        pos = pos.above();
        Self::place_basalt_pillar_base_hangoff(region, random, basalt, pos.north());
        Self::place_basalt_pillar_base_hangoff(region, random, basalt, pos.south());
        Self::place_basalt_pillar_base_hangoff(region, random, basalt, pos.west());
        Self::place_basalt_pillar_base_hangoff(region, random, basalt, pos.east());
        pos = pos.below();

        for dx in -3i32..4 {
            for dz in -3i32..4 {
                let probability = dx.abs() * dz.abs();
                if random.next_i32_bounded(10) < 10 - probability {
                    let mut base_pos = pos.offset(dx, 0, dz);
                    let mut max_drop = 3;

                    while region.block_state(base_pos.below()).is_air() {
                        base_pos = base_pos.below();
                        max_drop -= 1;
                        if max_drop <= 0 {
                            break;
                        }
                    }

                    if !region.block_state(base_pos.below()).is_air() {
                        let _ =
                            region.set_block_state(base_pos, basalt, UpdateFlags::UPDATE_CLIENTS);
                    }
                }
            }
        }

        true
    }

    fn place_basalt_pillar_base_hangoff(
        region: &mut WorldGenRegion<'_>,
        random: &mut Xoroshiro,
        basalt: BlockStateId,
        pos: BlockPos,
    ) {
        if random.next_bool() {
            let _ = region.set_block_state(pos, basalt, UpdateFlags::UPDATE_CLIENTS);
        }
    }

    fn place_basalt_pillar_hangoff(
        region: &mut WorldGenRegion<'_>,
        random: &mut Xoroshiro,
        basalt: BlockStateId,
        pos: BlockPos,
    ) -> bool {
        if random.next_i32_bounded(10) == 0 {
            return false;
        }

        let _ = region.set_block_state(pos, basalt, UpdateFlags::UPDATE_CLIENTS);
        true
    }

    fn place_simple_block_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &SimpleBlockConfiguration,
        origin: BlockPos,
    ) -> bool {
        let Some(state_to_place) = Self::sample_block_state_provider_optional(
            region,
            registry,
            random,
            &config.to_place,
            origin,
        ) else {
            return false;
        };

        let behavior = BLOCK_BEHAVIORS.get_behavior(state_to_place.get_block());
        if !behavior.can_survive(state_to_place, region, origin) {
            return false;
        }

        if Self::is_double_plant_block(state_to_place.get_block()) {
            if !region.block_state(origin.above()).is_air() {
                return false;
            }
            Self::place_double_plant(region, state_to_place, origin);
        } else if state_to_place.get_block() == &vanilla_blocks::PALE_MOSS_CARPET {
            Self::place_mossy_carpet(region, random, origin);
        } else {
            let _ = region.set_block_state(origin, state_to_place, UpdateFlags::UPDATE_CLIENTS);
        }

        if config.schedule_tick {
            let placed_state = region.block_state(origin);
            let _ = region.schedule_block_tick_default(origin, placed_state.get_block(), 1);
        }

        true
    }

    fn sample_block_state_provider_optional(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        provider: &BlockStateProvider,
        pos: BlockPos,
    ) -> Option<BlockStateId> {
        match provider {
            BlockStateProvider::RuleBased { fallback, rules } => {
                for rule in rules {
                    if Self::test_block_predicate(region, registry, &rule.if_true, pos) {
                        return Some(Self::sample_block_state_provider(
                            region, registry, random, &rule.then, pos,
                        ));
                    }
                }

                fallback.as_ref().map(|fallback| {
                    Self::sample_block_state_provider(region, registry, random, fallback, pos)
                })
            }
            _ => Some(Self::sample_block_state_provider(
                region, registry, random, provider, pos,
            )),
        }
    }

    fn sample_block_state_provider(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        provider: &BlockStateProvider,
        pos: BlockPos,
    ) -> BlockStateId {
        match provider {
            BlockStateProvider::Simple { state } => Self::block_state_from_data(registry, state),
            BlockStateProvider::Weighted { entries } => {
                assert!(
                    !entries.is_empty(),
                    "weighted block-state provider must not be empty"
                );
                let total_weight = entries.iter().fold(0, |total, entry| {
                    assert!(
                        entry.weight > 0,
                        "weighted block-state provider entry weight must be positive, got {}",
                        entry.weight
                    );
                    total + entry.weight
                });
                let mut target = random.next_i32_bounded(total_weight);
                for entry in entries {
                    if target < entry.weight {
                        return Self::block_state_from_data(registry, &entry.data);
                    }
                    target -= entry.weight;
                }
                panic!("weighted block-state provider failed to select an entry");
            }
            BlockStateProvider::RotatedBlock { state } => {
                let state = Self::block_state_from_data(registry, state);
                state.set_value(&BlockStateProperties::AXIS, Self::random_axis(random))
            }
            BlockStateProvider::RandomizedInt {
                property,
                source,
                values,
            } => {
                let state =
                    Self::sample_block_state_provider(region, registry, random, source, pos);
                let value = values.sample(random);
                Self::set_int_property_by_name(registry, state, property, value)
            }
            BlockStateProvider::RuleBased { .. } => {
                if let Some(state) = Self::sample_block_state_provider_optional(
                    region, registry, random, provider, pos,
                ) {
                    state
                } else {
                    region.block_state(pos)
                }
            }
            BlockStateProvider::Noise(provider) => {
                Self::sample_noise_provider(registry, provider, pos)
            }
            BlockStateProvider::NoiseThreshold(provider) => {
                Self::sample_noise_threshold_provider(registry, random, provider, pos)
            }
            BlockStateProvider::DualNoise(provider) => {
                Self::sample_dual_noise_provider(registry, provider, pos)
            }
        }
    }

    fn random_axis(random: &mut Xoroshiro) -> Axis {
        match random.next_i32_bounded(3) {
            0 => Axis::X,
            1 => Axis::Y,
            _ => Axis::Z,
        }
    }

    fn set_int_property_by_name(
        registry: &Registry,
        state: BlockStateId,
        property: &str,
        value: i32,
    ) -> BlockStateId {
        let value_string = value.to_string();
        let current_properties = registry.blocks.get_properties(state);
        let mut found = false;
        let properties = current_properties
            .iter()
            .map(|(name, existing)| {
                if *name == property {
                    found = true;
                    (*name, value_string.as_str())
                } else {
                    (*name, *existing)
                }
            })
            .collect::<Vec<_>>();

        if !found {
            return state;
        }

        let Some(new_state) = registry
            .blocks
            .state_id_from_properties(&state.get_block().key, &properties)
        else {
            panic!(
                "randomized int provider produced invalid value {value} for property {property} on {}",
                state.get_block().key
            );
        };
        new_state
    }

    fn sample_noise_provider(
        registry: &Registry,
        provider: &NoiseProvider,
        pos: BlockPos,
    ) -> BlockStateId {
        let noise = Self::normal_noise(&provider.noise, provider.seed);
        let noise_value = Self::noise_value(&noise, pos, provider.scale);
        Self::noise_state_by_value(registry, &provider.states, noise_value)
    }

    fn sample_noise_threshold_provider(
        registry: &Registry,
        random: &mut Xoroshiro,
        provider: &NoiseThresholdProvider,
        pos: BlockPos,
    ) -> BlockStateId {
        let noise = Self::normal_noise(&provider.noise, provider.seed);
        let noise_value = Self::noise_value(&noise, pos, provider.scale);
        if noise_value < provider.threshold {
            Self::random_block_state_from_data_list(registry, random, &provider.low_states)
        } else if random.next_f32() < provider.high_chance {
            Self::random_block_state_from_data_list(registry, random, &provider.high_states)
        } else {
            Self::block_state_from_data(registry, &provider.default_state)
        }
    }

    fn sample_dual_noise_provider(
        registry: &Registry,
        provider: &DualNoiseProvider,
        pos: BlockPos,
    ) -> BlockStateId {
        let slow_noise = Self::normal_noise(&provider.slow_noise, provider.seed);
        let variety_noise = Self::noise_value(&slow_noise, pos, provider.slow_scale);
        let local_variety = Self::clamped_map(
            variety_noise,
            -1.0,
            1.0,
            f64::from(provider.variety[0]),
            f64::from(provider.variety[1] + 1),
        ) as i32;
        assert!(
            local_variety > 0,
            "dual-noise provider local variety must be positive, got {local_variety}"
        );

        let Ok(capacity) = usize::try_from(local_variety) else {
            panic!("dual-noise provider local variety {local_variety} exceeds usize range");
        };
        let mut possible_states = Vec::with_capacity(capacity);
        for i in 0..local_variety {
            let offset_pos = pos.offset(i * 54_545, 0, i * 34_234);
            let slow_value = Self::noise_value(&slow_noise, offset_pos, provider.slow_scale);
            possible_states.push(Self::noise_state_by_value(
                registry,
                &provider.states,
                slow_value,
            ));
        }

        let noise = Self::normal_noise(&provider.noise, provider.seed);
        let noise_value = Self::noise_value(&noise, pos, provider.scale);
        Self::noise_state_by_resolved_value(&possible_states, noise_value)
    }

    fn normal_noise(parameters: &FeatureNoiseParameters, seed: i64) -> NormalNoise {
        let mut random = RandomSource::Legacy(LegacyRandom::from_seed(seed as u64));
        NormalNoise::create_from_random(
            &mut random,
            parameters.first_octave,
            &parameters.amplitudes,
        )
    }

    fn noise_value(noise: &NormalNoise, pos: BlockPos, scale: f64) -> f64 {
        noise.get_value(
            f64::from(pos.x()) * scale,
            f64::from(pos.y()) * scale,
            f64::from(pos.z()) * scale,
        )
    }

    fn noise_state_by_value(
        registry: &Registry,
        states: &[BlockStateData],
        noise_value: f64,
    ) -> BlockStateId {
        assert!(
            !states.is_empty(),
            "noise provider state list must not be empty"
        );
        let index = Self::noise_state_index(states.len(), noise_value);
        Self::block_state_from_data(registry, &states[index])
    }

    fn noise_state_by_resolved_value(states: &[BlockStateId], noise_value: f64) -> BlockStateId {
        assert!(
            !states.is_empty(),
            "noise provider state list must not be empty"
        );
        states[Self::noise_state_index(states.len(), noise_value)]
    }

    fn noise_state_index(state_count: usize, noise_value: f64) -> usize {
        let placement_value = ((1.0 + noise_value) / 2.0).clamp(0.0, 0.9999);
        (placement_value * state_count as f64) as usize
    }

    fn random_block_state_from_data_list(
        registry: &Registry,
        random: &mut Xoroshiro,
        states: &[BlockStateData],
    ) -> BlockStateId {
        assert!(
            !states.is_empty(),
            "random block-state provider state list must not be empty"
        );
        let Ok(state_count) = i32::try_from(states.len()) else {
            panic!(
                "random block-state provider state count {} exceeds i32 range",
                states.len()
            );
        };
        let index = random.next_i32_bounded(state_count) as usize;
        Self::block_state_from_data(registry, &states[index])
    }

    fn clamped_map(value: f64, from_low: f64, from_high: f64, to_low: f64, to_high: f64) -> f64 {
        let inverse_lerp = ((value - from_low) / (from_high - from_low)).clamp(0.0, 1.0);
        to_low + inverse_lerp * (to_high - to_low)
    }

    fn is_double_plant_block(block: BlockRef) -> bool {
        block == &vanilla_blocks::SUNFLOWER
            || block == &vanilla_blocks::LILAC
            || block == &vanilla_blocks::ROSE_BUSH
            || block == &vanilla_blocks::PEONY
            || block == &vanilla_blocks::TALL_GRASS
            || block == &vanilla_blocks::LARGE_FERN
            || block == &vanilla_blocks::PITCHER_PLANT
            || block == &vanilla_blocks::SMALL_DRIPLEAF
    }

    fn place_double_plant(
        region: &mut WorldGenRegion<'_>,
        state: BlockStateId,
        lower_pos: BlockPos,
    ) {
        let upper_pos = lower_pos.above();
        let lower_state = Self::copy_waterlogged_from(
            region,
            lower_pos,
            state.set_value(
                &BlockStateProperties::DOUBLE_BLOCK_HALF,
                DoubleBlockHalf::Lower,
            ),
        );
        let upper_state = Self::copy_waterlogged_from(
            region,
            upper_pos,
            state.set_value(
                &BlockStateProperties::DOUBLE_BLOCK_HALF,
                DoubleBlockHalf::Upper,
            ),
        );
        let _ = region.set_block_state(lower_pos, lower_state, UpdateFlags::UPDATE_CLIENTS);
        let _ = region.set_block_state(upper_pos, upper_state, UpdateFlags::UPDATE_CLIENTS);
    }

    fn copy_waterlogged_from(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockStateId {
        if state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .is_none()
        {
            return state;
        }

        let waterlogged = get_fluid_state_from_block(region.block_state(pos)).is_water();
        state.set_value(&BlockStateProperties::WATERLOGGED, waterlogged)
    }

    fn place_mossy_carpet(region: &mut WorldGenRegion<'_>, random: &mut Xoroshiro, pos: BlockPos) {
        let simple_carpet_layer = vanilla_blocks::PALE_MOSS_CARPET.default_state();
        let adjusted_carpet_layer =
            Self::updated_mossy_carpet_state(region, simple_carpet_layer, pos, true);
        let _ = region.set_block_state(pos, adjusted_carpet_layer, UpdateFlags::UPDATE_CLIENTS);

        let topper = Self::create_mossy_carpet_topper(region, random, pos);
        if !topper.is_air() {
            let _ = region.set_block_state(pos.above(), topper, UpdateFlags::UPDATE_CLIENTS);
            let update_bottom =
                Self::updated_mossy_carpet_state(region, adjusted_carpet_layer, pos, true);
            let _ = region.set_block_state(pos, update_bottom, UpdateFlags::UPDATE_CLIENTS);
        }
    }

    fn create_mossy_carpet_topper(
        region: &WorldGenRegion<'_>,
        random: &mut Xoroshiro,
        pos: BlockPos,
    ) -> BlockStateId {
        let above = pos.above();
        let above_previous_state = region.block_state(above);
        let is_mossy_carpet_above =
            above_previous_state.get_block() == &vanilla_blocks::PALE_MOSS_CARPET;
        if (!is_mossy_carpet_above
            || !above_previous_state.get_value(&BlockStateProperties::BOTTOM))
            && (is_mossy_carpet_above || above_previous_state.is_replaceable())
        {
            let no_base_state = vanilla_blocks::PALE_MOSS_CARPET
                .default_state()
                .set_value(&BlockStateProperties::BOTTOM, false);
            let mut above_state =
                Self::updated_mossy_carpet_state(region, no_base_state, above, true);

            for direction in Self::HORIZONTAL_DIRECTIONS {
                let property = Self::mossy_carpet_wall_property(direction);
                if above_state.get_value(&property) != WallSide::None && !random.next_bool() {
                    above_state = above_state.set_value(&property, WallSide::None);
                }
            }

            if Self::mossy_carpet_has_faces(above_state) && above_state != above_previous_state {
                above_state
            } else {
                vanilla_blocks::AIR.default_state()
            }
        } else {
            vanilla_blocks::AIR.default_state()
        }
    }

    fn updated_mossy_carpet_state(
        region: &WorldGenRegion<'_>,
        mut state: BlockStateId,
        pos: BlockPos,
        create_sides: bool,
    ) -> BlockStateId {
        let create_sides = create_sides || state.get_value(&BlockStateProperties::BOTTOM);

        for direction in Self::HORIZONTAL_DIRECTIONS {
            let property = Self::mossy_carpet_wall_property(direction);
            let mut side = if Self::mossy_carpet_can_support_at_face(region, pos, direction) {
                if create_sides {
                    WallSide::Low
                } else {
                    state.get_value(&property)
                }
            } else {
                WallSide::None
            };

            if side == WallSide::Low {
                let above_state = region.block_state(pos.above());
                if above_state.get_block() == &vanilla_blocks::PALE_MOSS_CARPET
                    && above_state.get_value(&property) != WallSide::None
                    && !above_state.get_value(&BlockStateProperties::BOTTOM)
                {
                    side = WallSide::Tall;
                }

                if !state.get_value(&BlockStateProperties::BOTTOM) {
                    let below_state = region.block_state(pos.below());
                    if below_state.get_block() == &vanilla_blocks::PALE_MOSS_CARPET
                        && below_state.get_value(&property) == WallSide::None
                    {
                        side = WallSide::None;
                    }
                }
            }

            state = state.set_value(&property, side);
        }

        state
    }

    const HORIZONTAL_DIRECTIONS: [Direction; 4] = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];

    fn mossy_carpet_wall_property(direction: Direction) -> EnumProperty<WallSide> {
        match direction {
            Direction::North => BlockStateProperties::NORTH_WALL,
            Direction::East => BlockStateProperties::EAST_WALL,
            Direction::South => BlockStateProperties::SOUTH_WALL,
            Direction::West => BlockStateProperties::WEST_WALL,
            Direction::Down | Direction::Up => {
                panic!("mossy carpet has no wall property for vertical direction")
            }
        }
    }

    fn mossy_carpet_can_support_at_face(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        direction: Direction,
    ) -> bool {
        direction != Direction::Up && Self::can_attach_to_multiface(region, pos, direction)
    }

    fn can_attach_to_multiface(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        direction_towards_neighbour: Direction,
    ) -> bool {
        let neighbour_pos = pos.relative(direction_towards_neighbour);
        let neighbour_state = region.block_state(neighbour_pos);
        let support_direction = direction_towards_neighbour.opposite();
        shapes::is_face_full(neighbour_state.get_support_shape(), support_direction)
            || shapes::is_face_full(neighbour_state.get_collision_shape(), support_direction)
    }

    fn mossy_carpet_has_faces(state: BlockStateId) -> bool {
        if state.get_value(&BlockStateProperties::BOTTOM) {
            return true;
        }

        for direction in Self::HORIZONTAL_DIRECTIONS {
            let property = Self::mossy_carpet_wall_property(direction);
            if state.get_value(&property) != WallSide::None {
                return true;
            }
        }

        false
    }

    fn configured_feature_kind<'a>(
        registry: &'a Registry,
        feature: &'a ConfiguredFeatureRef,
    ) -> &'a ConfiguredFeatureKind {
        match feature {
            ConfiguredFeatureRef::Reference(key) => {
                let Some(configured_feature) = registry.configured_features.by_key(key) else {
                    panic!("placed feature references unknown configured feature {key}");
                };
                &configured_feature.kind
            }
            ConfiguredFeatureRef::Inline(configured_feature) => configured_feature,
        }
    }

    const fn feature_heightmap_type(heightmap: FeatureHeightmap) -> HeightmapType {
        match heightmap {
            FeatureHeightmap::WorldSurface => HeightmapType::WorldSurface,
            FeatureHeightmap::MotionBlocking => HeightmapType::MotionBlocking,
            FeatureHeightmap::MotionBlockingNoLeaves => HeightmapType::MotionBlockingNoLeaves,
            FeatureHeightmap::OceanFloor => HeightmapType::OceanFloor,
            FeatureHeightmap::WorldSurfaceWg => HeightmapType::WorldSurfaceWg,
            FeatureHeightmap::OceanFloorWg => HeightmapType::OceanFloorWg,
        }
    }

    fn repeat_position(origin: BlockPos, count: i32) -> Vec<BlockPos> {
        let Ok(count) = usize::try_from(count) else {
            return Vec::new();
        };
        vec![origin; count]
    }

    fn count_on_every_layer_positions(
        region: &WorldGenRegion<'_>,
        random: &mut Xoroshiro,
        origin: BlockPos,
        count: &IntProvider,
    ) -> Vec<BlockPos> {
        let mut positions = Vec::new();
        let mut layer = 0;

        loop {
            let mut found_any = false;
            for _ in 0..count.sample(random) {
                let x = origin.x() + random.next_i32_bounded(16);
                let z = origin.z() + random.next_i32_bounded(16);
                let start_y = region.height_at(HeightmapType::MotionBlocking, x, z);
                if let Some(y) = Self::find_on_ground_y_position(region, x, start_y, z, layer) {
                    positions.push(BlockPos::new(x, y, z));
                    found_any = true;
                }
            }

            if !found_any {
                break;
            }
            layer += 1;
        }

        positions
    }

    fn find_on_ground_y_position(
        region: &WorldGenRegion<'_>,
        x: i32,
        start_y: i32,
        z: i32,
        layer_to_place_on: i32,
    ) -> Option<i32> {
        let mut current_layer = 0;
        let mut current_block = region.block_state(BlockPos::new(x, start_y, z));

        for y in (region.min_y() + 1..=start_y).rev() {
            let below_block = region.block_state(BlockPos::new(x, y - 1, z));
            if !Self::is_empty_layer_block(below_block)
                && Self::is_empty_layer_block(current_block)
                && below_block.get_block() != &vanilla_blocks::BEDROCK
            {
                if current_layer == layer_to_place_on {
                    return Some(y);
                }
                current_layer += 1;
            }

            current_block = below_block;
        }

        None
    }

    fn is_empty_layer_block(state: steel_utils::BlockStateId) -> bool {
        state.is_air()
            || state.get_block() == &vanilla_blocks::WATER
            || state.get_block() == &vanilla_blocks::LAVA
    }

    fn environment_scan_positions(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        origin: BlockPos,
        direction_of_search: steel_utils::Direction,
        target_condition: &BlockPredicate,
        allowed_search_condition: Option<&BlockPredicate>,
        max_steps: i32,
    ) -> Vec<BlockPos> {
        assert!(
            max_steps > 0,
            "environment scan max_steps must be positive, got {max_steps}"
        );

        let mut position = origin;
        if !Self::test_optional_block_predicate(
            region,
            registry,
            allowed_search_condition,
            position,
        ) {
            return Vec::new();
        }

        for _ in 0..max_steps {
            if Self::test_block_predicate(region, registry, target_condition, position) {
                return vec![position];
            }

            position = position.relative(direction_of_search);
            if region.is_outside_build_height(position.y()) {
                return Vec::new();
            }

            if !Self::test_optional_block_predicate(
                region,
                registry,
                allowed_search_condition,
                position,
            ) {
                break;
            }
        }

        if Self::test_block_predicate(region, registry, target_condition, position) {
            vec![position]
        } else {
            Vec::new()
        }
    }

    fn test_optional_block_predicate(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        predicate: Option<&BlockPredicate>,
        origin: BlockPos,
    ) -> bool {
        predicate
            .is_none_or(|predicate| Self::test_block_predicate(region, registry, predicate, origin))
    }

    fn biome_allows_feature(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        biome_zoom_seed: i64,
        origin: BlockPos,
        biome_filter_feature_id: Option<usize>,
    ) -> bool {
        let biome_id = fuzzed_biome_at_block(
            biome_zoom_seed,
            origin.x(),
            origin.y(),
            origin.z(),
            |quart_x, quart_y, quart_z| region.noise_biome_id(quart_x, quart_y, quart_z),
        );
        let Some(biome) = registry.biomes.by_id(usize::from(biome_id)) else {
            panic!("biome filter resolved unknown biome id {biome_id}");
        };
        let Some(feature_id) = biome_filter_feature_id else {
            panic!(
                "Tried to biome check an unregistered feature, or a feature that should not restrict the biome"
            );
        };

        biome.features.iter().flatten().any(|feature_key| {
            registry.placed_features.id_from_key(feature_key) == Some(feature_id)
        })
    }

    fn test_block_predicate(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        predicate: &BlockPredicate,
        origin: BlockPos,
    ) -> bool {
        match predicate {
            BlockPredicate::AllOf { predicates } => predicates
                .iter()
                .all(|predicate| Self::test_block_predicate(region, registry, predicate, origin)),
            BlockPredicate::AnyOf { predicates } => predicates
                .iter()
                .any(|predicate| Self::test_block_predicate(region, registry, predicate, origin)),
            BlockPredicate::Not { predicate } => {
                !Self::test_block_predicate(region, registry, predicate, origin)
            }
            BlockPredicate::MatchingBlockTag { tag, offset } => {
                let state = region.block_state(Self::offset(origin, offset));
                registry.blocks.is_in_tag(state.get_block(), tag)
            }
            BlockPredicate::MatchingBlocks { blocks, offset } => {
                let state = region.block_state(Self::offset(origin, offset));
                blocks.0.iter().any(|block_key| {
                    let Some(block) = registry.blocks.by_key(block_key) else {
                        panic!("block predicate references unknown block {block_key}");
                    };
                    state.get_block() == block
                })
            }
            BlockPredicate::MatchingFluids { fluids, offset } => {
                let state = region.block_state(Self::offset(origin, offset));
                let fluid_state = get_fluid_state_from_block(state);
                fluids.0.iter().any(|fluid_key| {
                    let Some(fluid) = registry.fluids.by_key(fluid_key) else {
                        panic!("block predicate references unknown fluid {fluid_key}");
                    };
                    fluid_state.fluid_id == fluid
                })
            }
            BlockPredicate::Solid { offset } => {
                region.block_state(Self::offset(origin, offset)).is_solid()
            }
            BlockPredicate::WouldSurvive { state, offset } => {
                let state = Self::block_state_from_data(registry, state);
                let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
                behavior.can_survive(state, region, Self::offset(origin, offset))
            }
            BlockPredicate::Replaceable { offset } => region
                .block_state(Self::offset(origin, offset))
                .is_replaceable(),
            BlockPredicate::HasSturdyFace { direction, offset } => region
                .block_state(Self::offset(origin, offset))
                .is_face_sturdy(*direction),
            BlockPredicate::InsideWorldBounds { offset } => {
                let position = Self::offset(origin, offset);
                !region.is_outside_build_height(position.y())
            }
        }
    }

    fn block_state_from_data(
        registry: &Registry,
        data: &BlockStateData,
    ) -> steel_utils::BlockStateId {
        let Some(block) = registry.blocks.by_key(&data.name) else {
            panic!(
                "block state provider references unknown block {}",
                data.name
            );
        };

        let mut properties = registry
            .blocks
            .get_properties(block.default_state())
            .into_iter()
            .map(|(key, value)| (key as &str, value as &str))
            .collect::<Vec<_>>();

        for (key, value) in &data.properties {
            let Some((_, property_value)) = properties
                .iter_mut()
                .find(|(property_key, _)| *property_key == key)
            else {
                panic!(
                    "block state provider references unknown property {key} on {}",
                    data.name
                );
            };
            *property_value = value.as_str();
        }

        let Some(state) = registry
            .blocks
            .state_id_from_properties(&data.name, &properties)
        else {
            panic!(
                "block state provider references unknown or invalid state {}",
                data.name
            );
        };
        state
    }

    const fn offset(origin: BlockPos, offset: &[i32; 3]) -> BlockPos {
        origin.offset(offset[0], offset[1], offset[2])
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::Registry;

    use crate::worldgen::BiomeSourceKind;

    use super::FeatureDecorationRunner;

    #[test]
    fn vanilla_feature_sorter_builds_for_all_builtin_biome_sources() {
        let mut registry = Registry::new_vanilla();
        registry.freeze();

        let sources = [
            BiomeSourceKind::overworld(0),
            BiomeSourceKind::nether(0),
            BiomeSourceKind::end(0),
        ];

        for source in sources {
            let possible_biomes = source.possible_biome_refs();
            let runner = FeatureDecorationRunner::new(&possible_biomes, &registry);
            assert!(runner.sorter.step_count() > 0);
        }
    }

    #[test]
    fn block_column_truncation_matches_vanilla_tip_priority() {
        let mut preserved_base = [2, 3, 4];
        FeatureDecorationRunner::truncate_block_column_layers(&mut preserved_base, 9, 6, false);
        assert_eq!(preserved_base, [2, 3, 1]);

        let mut preserved_tip = [2, 3, 4];
        FeatureDecorationRunner::truncate_block_column_layers(&mut preserved_tip, 9, 6, true);
        assert_eq!(preserved_tip, [0, 2, 4]);
    }
}
