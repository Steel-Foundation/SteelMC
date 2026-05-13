//! Biome decoration runner for the `FEATURES` chunk stage.
//!
//! Vanilla treats biome decoration as one ordered pass over structure pieces and placed
//! features. This module builds the same per-step placed-feature ordering up front and
//! drives the per-chunk decoration seed loop. Actual placed/configured feature execution is
//! intentionally a no-op until the individual feature implementations exist.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::{FxHashMap, FxHashSet};
use steel_registry::biome::BiomeRef;
use steel_registry::feature::PlacedFeatureEntryRef;
use steel_registry::{Registry, RegistryEntry as _, RegistryExt as _};
use steel_utils::BlockPos;
use steel_utils::random::xoroshiro::Xoroshiro;

use crate::chunk::chunk_access::ChunkStatus;
use crate::worldgen::region::WorldGenRegion;

const DECORATION_STEP_COUNT: usize = 11;

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

    pub(crate) fn decorate(&self, region: &mut WorldGenRegion<'_>, registry: &Registry, seed: i64) {
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
            Self::place_placed_feature(region, registry, random, origin, feature);
        }
    }

    const fn place_placed_feature(
        _region: &mut WorldGenRegion<'_>,
        _registry: &Registry,
        _random: &mut Xoroshiro,
        _origin: BlockPos,
        _feature: PlacedFeatureEntryRef,
    ) -> bool {
        // TODO: Execute placement modifiers and configured feature placement.
        false
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
