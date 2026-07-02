use glam::IVec3;
use smallvec::SmallVec;
use steel_registry::biome::BiomeRef;
use steel_registry::structure::StructureRef;
use steel_utils::{BoundingBox, ChunkPos};

use super::prelude::*;
use super::sorter::{FeatureSorter, FeatureStepData};
use crate::worldgen::structure_piece_placer::StructurePiecePlacer;
#[cfg(test)]
use steel_worldgen::structure::StructureReferenceMap;
use steel_worldgen::structure::StructureStart;

/// Runs the structure-piece and placed-feature decoration pass for a generator.
#[derive(Debug)]
pub(crate) struct FeatureDecorationRunner {
    pub(super) sorter: FeatureSorter,
    source_biome_lookup: Box<[bool]>,
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

    pub(super) fn random_horizontal_direction(random: &mut WorldgenRandom) -> Direction {
        Self::VANILLA_HORIZONTAL_DIRECTIONS[random.next_i32_bounded(4) as usize]
    }

    pub(super) fn shuffled_directions<const N: usize>(
        random: &mut WorldgenRandom,
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

    pub(super) fn for_each_vanilla_between_closed(
        min: BlockPos,
        max: BlockPos,
        mut visitor: impl FnMut(BlockPos),
    ) {
        let width = max.x() - min.x() + 1;
        let height = max.y() - min.y() + 1;
        let depth = max.z() - min.z() + 1;
        debug_assert!(width > 0 && height > 0 && depth > 0);

        let end = i64::from(width) * i64::from(height) * i64::from(depth);
        for index in 0..end {
            let x = (index % i64::from(width)) as i32;
            let slice = index / i64::from(width);
            let y = (slice % i64::from(height)) as i32;
            let z = (slice / i64::from(height)) as i32;
            visitor(BlockPos::new(min.x() + x, min.y() + y, min.z() + z));
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
        let mut max_biome_id = 0;

        for &biome in possible_biomes {
            let Some(biome_id) = biome.try_id() else {
                panic!("possible biome {} is not registered", biome.key);
            };
            max_biome_id = max_biome_id.max(biome_id);

            if source_biome_ids.insert(biome_id) {
                unique_biomes.push(biome);
            }
        }

        let mut source_biome_lookup = vec![false; max_biome_id + 1].into_boxed_slice();
        for biome_id in source_biome_ids {
            source_biome_lookup[biome_id] = true;
        }

        Self {
            sorter: FeatureSorter::build(&unique_biomes, registry),
            source_biome_lookup,
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

        let mut random = WorldgenRandom::from_seed(0);
        let decoration_seed = random.set_decoration_seed(seed, origin.x(), origin.z());
        let step_count = DECORATION_STEP_COUNT.max(self.sorter.step_count());

        for step in 0..step_count {
            Self::place_structures_for_step(
                region,
                registry,
                decoration_seed,
                &mut random,
                step,
                biome_zoom_seed,
            );

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
        let mut seen = vec![false; self.source_biome_lookup.len()];
        let mut biomes = Vec::new();

        for chunk_z in center.0.y - 1..=center.0.y + 1 {
            for chunk_x in center.0.x - 1..=center.0.x + 1 {
                let chunk = region.chunk(chunk_x, chunk_z, ChunkStatus::Biomes);
                chunk.sections().for_each_biome_id(|biome_id| {
                    let biome_id = usize::from(biome_id);
                    if self
                        .source_biome_lookup
                        .get(biome_id)
                        .copied()
                        .unwrap_or(false)
                        && !seen[biome_id]
                    {
                        seen[biome_id] = true;
                        biomes.push(biome_id);
                    }
                });
            }
        }

        biomes.sort_unstable();
        biomes
    }

    pub(super) fn structures_for_decoration_step(
        registry: &Registry,
        step: usize,
    ) -> Vec<StructureRef> {
        registry
            .structures
            .iter()
            .map(|(_, structure)| structure)
            .filter(|structure| structure.step.decoration_ordinal() == step)
            .collect()
    }

    pub(super) const fn center_chunk_writable_box(region: &WorldGenRegion<'_>) -> BoundingBox {
        Self::chunk_writable_box(region.center(), region.min_y(), region.max_y_exclusive())
    }

    pub(super) const fn chunk_writable_box(
        center: ChunkPos,
        min_y: i32,
        max_y_exclusive: i32,
    ) -> BoundingBox {
        let min_x = center.0.x * 16;
        let min_z = center.0.y * 16;
        BoundingBox::new(
            IVec3::new(min_x, min_y + 1, min_z),
            IVec3::new(min_x + 15, max_y_exclusive - 1, min_z + 15),
        )
    }

    #[cfg(test)]
    pub(super) fn resolve_structure_starts_from_references(
        references: &StructureReferenceMap,
        structure_id: &Identifier,
        mut start_lookup: impl FnMut(steel_utils::ChunkPos, &Identifier) -> Option<StructureStart>,
    ) -> Vec<StructureStart> {
        let Some(source_positions) = references.get(structure_id) else {
            return Vec::new();
        };

        let mut starts = Vec::new();
        for &source_pos in source_positions {
            let Some(start) = start_lookup(source_pos, structure_id) else {
                continue;
            };
            if start.chunk_pos == source_pos && !start.pieces.is_empty() {
                starts.push(start);
            }
        }
        starts
    }

    fn structure_source_positions_in_region(
        region: &WorldGenRegion<'_>,
        structure_id: &Identifier,
    ) -> Vec<steel_utils::ChunkPos> {
        let center = region.center();
        let center_chunk = region.chunk(center.0.x, center.0.y, ChunkStatus::StructureStarts);
        let references = center_chunk.structure_references();
        let source_positions = references
            .get(structure_id)
            .map(|positions| positions.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        drop(references);
        drop(center_chunk);
        source_positions
    }

    pub(super) fn place_structures_for_step(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        decoration_seed: i64,
        random: &mut WorldgenRandom,
        step: usize,
        biome_zoom_seed: i64,
    ) {
        let writable_box = Self::center_chunk_writable_box(region);

        for (structure_index, structure) in Self::structures_for_decoration_step(registry, step)
            .into_iter()
            .enumerate()
        {
            Self::set_structure_seed(random, decoration_seed, structure_index, step);

            let source_positions =
                Self::structure_source_positions_in_region(region, &structure.key);
            for source_pos in source_positions {
                let Some(source_chunk) =
                    region.try_chunk(source_pos.0.x, source_pos.0.y, ChunkStatus::StructureStarts)
                else {
                    continue;
                };
                let mut source_starts = source_chunk.structure_starts_mut();
                let Some(start) = source_starts.get_mut(&structure.key) else {
                    continue;
                };
                if start.chunk_pos != source_pos || start.pieces.is_empty() {
                    continue;
                }
                let Some(reference_pos) = start.placement_reference_pos() else {
                    continue;
                };
                for piece in &mut start.pieces {
                    if piece.bounding_box.intersects(writable_box) {
                        StructurePiecePlacer::place_piece(
                            region,
                            registry,
                            piece,
                            reference_pos,
                            writable_box,
                            random,
                            biome_zoom_seed,
                        );
                    }
                }
                StructurePiecePlacer::after_place_structure(
                    region,
                    structure,
                    &mut start.pieces,
                    writable_box,
                );
                start.bounding_box =
                    StructureStart::compute_bounding_box(&start.pieces, start.bb_inflate);
            }
        }
    }

    pub(super) fn set_structure_seed(
        random: &mut WorldgenRandom,
        decoration_seed: i64,
        structure_index: usize,
        step: usize,
    ) {
        let Ok(structure_index_i32) = i32::try_from(structure_index) else {
            panic!("structure index {structure_index} exceeds i32 range");
        };
        let Ok(step_i32) = i32::try_from(step) else {
            panic!("decoration step {step} exceeds i32 range");
        };
        random.set_feature_seed(decoration_seed, structure_index_i32, step_i32);
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors vanilla's decoration loop state without hiding generation inputs"
    )]
    pub(super) fn place_features_for_step(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        decoration_seed: i64,
        random: &mut WorldgenRandom,
        origin: BlockPos,
        step: usize,
        step_features: &FeatureStepData,
        possible_biomes: &[usize],
        biome_zoom_seed: i64,
    ) {
        let mut feature_indices = SmallVec::<[usize; 64]>::new();

        for &biome_id in possible_biomes {
            if let Some(indices) = step_features.feature_indices_for_biome(biome_id) {
                feature_indices.extend_from_slice(indices);
            }
        }

        feature_indices.sort_unstable();
        feature_indices.dedup();

        for feature_index in feature_indices {
            let Ok(feature_index_i32) = i32::try_from(feature_index) else {
                panic!("decoration feature index {feature_index} exceeds i32 range");
            };
            let Ok(step_i32) = i32::try_from(step) else {
                panic!("decoration step {step} exceeds i32 range");
            };
            let Some(feature) = step_features.feature(feature_index) else {
                panic!("decoration step {step} references missing feature index {feature_index}");
            };
            random.set_feature_seed(decoration_seed, feature_index_i32, step_i32);
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
