//! Biome decoration runner for the `FEATURES` chunk stage.
//!
//! Vanilla treats biome decoration as one ordered pass over structure pieces and placed
//! features. This module builds the same per-step placed-feature ordering up front and
//! drives the per-chunk decoration seed loop. Placed-feature modifiers execute normally;
//! configured feature placement is the explicit no-op boundary until individual feature
//! implementations are added.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use rustc_hash::{FxHashMap, FxHashSet};
use steel_registry::biome::BiomeRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::feature::{
    BlockPredicate, BlockStateData, ConfiguredFeatureKind, ConfiguredFeatureRef, FeatureHeightmap,
    PlacedFeatureEntryRef, PlacementModifier,
};
use steel_registry::{
    Registry, RegistryEntry as _, RegistryExt as _, TaggedRegistryExt as _, vanilla_blocks,
};
use steel_utils::random::{
    Random as _, RandomSource, legacy_random::LegacyRandom, xoroshiro::Xoroshiro,
};
use steel_utils::value_providers::IntProvider;
use steel_utils::{BlockPos, SectionPos};
use steel_worldgen::noise::PerlinSimplexNoise;

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
            Self::place_placed_feature(region, registry, random, origin, feature, biome_zoom_seed);
        }
    }

    fn place_placed_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        origin: BlockPos,
        feature: PlacedFeatureEntryRef,
        biome_zoom_seed: i64,
    ) -> bool {
        let mut positions = vec![origin];

        for modifier in &feature.data.placement {
            let mut next_positions = Vec::new();
            for position in positions {
                next_positions.extend(Self::apply_placement_modifier(
                    region,
                    registry,
                    random,
                    position,
                    feature,
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
                &feature.data.feature,
                position,
            );
        }
        placed
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
        feature: PlacedFeatureEntryRef,
        modifier: &PlacementModifier,
        biome_zoom_seed: i64,
    ) -> Vec<BlockPos> {
        match modifier {
            PlacementModifier::Biome => {
                if Self::biome_allows_feature(region, registry, biome_zoom_seed, origin, feature) {
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
        _region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        _random: &mut Xoroshiro,
        feature: &ConfiguredFeatureRef,
        _origin: BlockPos,
    ) -> bool {
        let _configured_feature = Self::configured_feature_kind(registry, feature);
        // TODO: Dispatch configured feature implementations as they are added.
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
        feature: PlacedFeatureEntryRef,
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
        let Some(feature_id) = feature.try_id() else {
            panic!(
                "biome filter received unregistered placed feature {}",
                feature.key
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
        let properties = data
            .properties
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>();
        let Some(state) = registry
            .blocks
            .state_id_from_properties(&data.name, &properties)
        else {
            panic!(
                "block predicate references unknown or invalid state {}",
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
}
