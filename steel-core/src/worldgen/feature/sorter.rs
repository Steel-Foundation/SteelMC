use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::FxHashMap;
use steel_registry::biome::BiomeRef;
use steel_registry::feature::PlacedFeatureEntryRef;
use steel_registry::{Registry, RegistryEntry as _, RegistryExt as _};

/// Cached vanilla ordering for all placed features reachable from a biome source.
#[derive(Debug)]
pub(super) struct FeatureSorter {
    steps: Box<[FeatureStepData]>,
}

#[derive(Debug)]
pub(super) struct FeatureStepData {
    features: Box<[PlacedFeatureEntryRef]>,
    index_by_placed_feature_id: FxHashMap<usize, usize>,
    feature_indices_by_biome_id: FxHashMap<usize, Box<[usize]>>,
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
    pub(super) fn build(possible_biomes: &[BiomeRef], registry: &Registry) -> Self {
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
        Self::from_sorted_features(&sorted_features, possible_biomes, registry)
    }

    #[must_use]
    pub(super) fn step_count(&self) -> usize {
        self.steps.len()
    }

    pub(super) fn step(&self, step: usize) -> Option<&FeatureStepData> {
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
    fn from_sorted_features(
        sorted_features: &[FeatureVertex],
        possible_biomes: &[BiomeRef],
        registry: &Registry,
    ) -> Self {
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
                feature_indices_by_biome_id: FxHashMap::default(),
            });
        }

        for biome in possible_biomes {
            let Some(biome_id) = biome.try_id() else {
                panic!("possible biome {} is not registered", biome.key);
            };

            for (step, feature_stage) in biome.features.iter().enumerate() {
                let Some(step_data) = steps.get_mut(step) else {
                    continue;
                };

                let mut indices = Vec::with_capacity(feature_stage.len());
                for feature_key in feature_stage {
                    let Some(placed_feature_id) = registry.placed_features.id_from_key(feature_key)
                    else {
                        panic!(
                            "biome {} references unknown placed feature {}",
                            biome.key, feature_key
                        );
                    };
                    let Some(feature_index) = step_data.feature_index(placed_feature_id) else {
                        panic!(
                            "placed feature {} from biome {} was not included in decoration step {}",
                            feature_key, biome.key, step
                        );
                    };
                    indices.push(feature_index);
                }

                if indices.is_empty() {
                    continue;
                }

                indices.sort_unstable();
                indices.dedup();
                step_data
                    .feature_indices_by_biome_id
                    .insert(biome_id, indices.into_boxed_slice());
            }
        }

        Self {
            steps: steps.into_boxed_slice(),
        }
    }
}

impl FeatureStepData {
    pub(super) fn feature_index(&self, placed_feature_id: usize) -> Option<usize> {
        self.index_by_placed_feature_id
            .get(&placed_feature_id)
            .copied()
    }

    pub(super) fn feature(&self, index: usize) -> Option<PlacedFeatureEntryRef> {
        self.features.get(index).copied()
    }

    pub(super) fn feature_indices_for_biome(&self, biome_id: usize) -> Option<&[usize]> {
        self.feature_indices_by_biome_id
            .get(&biome_id)
            .map(Box::as_ref)
    }
}
