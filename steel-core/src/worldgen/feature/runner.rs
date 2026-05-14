use steel_registry::biome::BiomeRef;

use super::prelude::*;
use super::sorter::{FeatureSorter, FeatureStepData};

/// Runs the structure-piece and placed-feature decoration pass for a generator.
#[derive(Debug)]
pub(crate) struct FeatureDecorationRunner {
    pub(super) sorter: FeatureSorter,
    source_biome_ids: FxHashSet<usize>,
}

impl FeatureDecorationRunner {
    pub(super) const VANILLA_DIRECTION_VALUES: [Direction; 6] = [
        Direction::Down,
        Direction::Up,
        Direction::North,
        Direction::South,
        Direction::West,
        Direction::East,
    ];

    pub(super) const VANILLA_HORIZONTAL_DIRECTIONS: [Direction; 4] = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];

    pub(super) fn random_horizontal_direction(random: &mut Xoroshiro) -> Direction {
        Self::VANILLA_HORIZONTAL_DIRECTIONS[random.next_i32_bounded(4) as usize]
    }

    pub(super) fn shuffled_directions<const N: usize>(
        random: &mut Xoroshiro,
        mut directions: [Direction; N],
    ) -> [Direction; N] {
        for i in (1..N).rev() {
            let Ok(bound) = i32::try_from(i + 1) else {
                panic!("direction shuffle length {N} exceeds i32 range");
            };
            let j = random.next_i32_bounded(bound) as usize;
            directions.swap(i, j);
        }
        directions
    }

    pub(super) const fn manhattan_distance(left: BlockPos, right: BlockPos) -> i32 {
        Self::abs_diff(left.x(), right.x())
            + Self::abs_diff(left.y(), right.y())
            + Self::abs_diff(left.z(), right.z())
    }

    pub(super) fn for_each_vanilla_within_manhattan(
        origin: BlockPos,
        reach_x: i32,
        reach_y: i32,
        reach_z: i32,
        mut visitor: impl FnMut(BlockPos) -> bool,
    ) {
        let max_depth = reach_x + reach_y + reach_z;
        for current_depth in 0..=max_depth {
            let max_x = reach_x.min(current_depth);
            for x in -max_x..=max_x {
                let max_y = reach_y.min(current_depth - x.abs());
                for y in -max_y..=max_y {
                    let z = current_depth - x.abs() - y.abs();
                    if z > reach_z {
                        continue;
                    }

                    if !visitor(origin.offset(x, y, z)) {
                        return;
                    }

                    if z != 0 && !visitor(origin.offset(x, y, -z)) {
                        return;
                    }
                }
            }
        }
    }

    const fn abs_diff(left: i32, right: i32) -> i32 {
        if left >= right {
            left - right
        } else {
            right - left
        }
    }

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

    pub(super) fn collect_possible_biome_ids(&self, region: &WorldGenRegion<'_>) -> Vec<usize> {
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

    pub(super) const fn place_structures_for_step(_region: &mut WorldGenRegion<'_>, _step: usize) {
        // TODO: Place generated structure pieces once template block payloads are extracted.
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors vanilla's decoration loop state without hiding generation inputs"
    )]
    pub(super) fn place_features_for_step(
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
}
