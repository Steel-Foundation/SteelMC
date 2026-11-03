use glam::IVec3;
use smallvec::SmallVec;
use steel_registry::biome::BiomeRef;
use steel_registry::structure::StructureRef;
use steel_utils::{BoundingBox, ChunkPos};

use super::prelude::*;
use super::sorter::{FeatureSorter, FeatureStepData};
use crate::worldgen::structure::piece_placer::StructurePiecePlacer;
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
        feature_trace::begin();

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
            .map_or_default(|positions| positions.iter().copied().collect::<Vec<_>>());
        drop(references);
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
                for (piece_ordinal, piece) in start.pieces.iter_mut().enumerate() {
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
                        feature_trace::record_piece(
                            region,
                            &structure.key,
                            piece_ordinal,
                            &piece.piece_type,
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
            feature_trace::record(region, "structure", &structure.key);
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
            feature_trace::record(region, "placed_feature", &feature.key);
        }
    }
}

/// Release builds carry no trace; see the test module below.
#[cfg(not(test))]
mod feature_trace {
    use steel_utils::Identifier;

    use crate::worldgen::region::WorldGenRegion;

    pub(super) const fn begin() {}

    pub(super) const fn record(_region: &WorldGenRegion<'_>, _kind: &str, _key: &Identifier) {}

    pub(super) const fn record_piece(
        _region: &WorldGenRegion<'_>,
        _structure: &Identifier,
        _piece_ordinal: usize,
        _piece_type: &Identifier,
    ) {
    }
}

/// Test-only mirror of the extractor's per-feature hash trace: with
/// `STEEL_FEATURE_TRACE_CHUNKS=x,z;x,z` set, prints the center chunk's block hash
/// after every structure and placed feature of its own decoration pass.
#[cfg(test)]
mod feature_trace {
    use std::cell::{Cell, RefCell};
    use std::env;
    use std::fs;
    use std::sync::LazyLock;

    use rustc_hash::{FxHashMap, FxHashSet};
    use steel_utils::{BlockPos, BlockStateId, Identifier, SectionPos};

    use crate::chunk::heightmap::HeightmapType;
    use crate::chunk::status::ChunkStatus;
    use crate::worldgen::chunk_stage_hashes::{compute_block_hash, describe_state};
    use crate::worldgen::region::{WorldGenChunkRef, WorldGenRegion};

    static TRACE_CHUNKS: LazyLock<FxHashSet<(i32, i32)>> = LazyLock::new(|| {
        env::var("STEEL_FEATURE_TRACE_CHUNKS")
            .ok()
            .map_or_else(FxHashSet::default, |value| {
                value
                    .split(';')
                    .filter_map(|pair| {
                        let (x, z) = pair.split_once(',')?;
                        Some((x.trim().parse().ok()?, z.trim().parse().ok()?))
                    })
                    .collect()
            })
    });

    /// `STEEL_FEATURE_TRACE_WATCH=x,y,z;x,y,z` additionally reports every change of
    /// those blocks, whichever chunk's pass made it.
    static WATCH_POSITIONS: LazyLock<Vec<BlockPos>> = LazyLock::new(|| {
        env::var("STEEL_FEATURE_TRACE_WATCH")
            .ok()
            .map_or_else(Vec::new, |value| {
                value
                    .split(';')
                    .filter_map(|triple| {
                        let mut parts = triple.split(',').map(|part| part.trim().parse::<i32>());
                        Some(BlockPos::new(
                            parts.next()?.ok()?,
                            parts.next()?.ok()?,
                            parts.next()?.ok()?,
                        ))
                    })
                    .collect()
            })
    });

    /// `STEEL_FEATURE_TRACE_WATCH_HEIGHT=x,z;x,z` reports every change of the
    /// `WORLD_SURFACE_WG` height of those columns as the region sees it.
    static WATCH_COLUMNS: LazyLock<Vec<(i32, i32)>> = LazyLock::new(|| {
        env::var("STEEL_FEATURE_TRACE_WATCH_HEIGHT")
            .ok()
            .map_or_else(Vec::new, |value| {
                value
                    .split(';')
                    .filter_map(|pair| {
                        let (x, z) = pair.split_once(',')?;
                        Some((x.trim().parse().ok()?, z.trim().parse().ok()?))
                    })
                    .collect()
            })
    });

    thread_local! {
        static INDEX: Cell<usize> = const { Cell::new(0) };
        static LAST_WATCHED: RefCell<FxHashMap<BlockPos, BlockStateId>> =
            RefCell::new(FxHashMap::default());
        static LAST_HEIGHTS: RefCell<FxHashMap<(i32, i32), i32>> =
            RefCell::new(FxHashMap::default());
    }

    fn report_watched_heights(
        region: &WorldGenRegion<'_>,
        pos: (i32, i32),
        index: usize,
        kind: &str,
        key: &Identifier,
    ) {
        for &(x, z) in WATCH_COLUMNS.iter() {
            let chunk_x = SectionPos::block_to_section_coord(x);
            let chunk_z = SectionPos::block_to_section_coord(z);
            if region
                .try_chunk(chunk_x, chunk_z, ChunkStatus::StructureStarts)
                .is_none()
            {
                continue;
            }
            let height = region.height_at(HeightmapType::WorldSurfaceWg, x, z);
            let changed = LAST_HEIGHTS.with(|last| {
                let mut last = last.borrow_mut();
                last.insert((x, z), height) != Some(height)
            });
            if changed {
                eprintln!(
                    "FEATURE_WATCH_HEIGHT ({x},{z}) pass=({},{}) index={index} {kind} {key}: WORLD_SURFACE_WG now {height}",
                    pos.0, pos.1
                );
            }
        }
    }

    fn report_watched(
        region: &WorldGenRegion<'_>,
        pos: (i32, i32),
        index: usize,
        kind: &str,
        key: &Identifier,
    ) {
        for &watched in WATCH_POSITIONS.iter() {
            let chunk_x = SectionPos::block_to_section_coord(watched.x());
            let chunk_z = SectionPos::block_to_section_coord(watched.z());
            if region
                .try_chunk(chunk_x, chunk_z, ChunkStatus::StructureStarts)
                .is_none()
            {
                continue;
            }
            let state = region.block_state(watched);
            let changed = LAST_WATCHED.with(|last| {
                let mut last = last.borrow_mut();
                let previous = last.insert(watched, state);
                previous != Some(state)
            });
            if changed {
                eprintln!(
                    "FEATURE_WATCH ({},{},{}) pass=({},{}) index={index} {kind} {key}: now {}",
                    watched.x(),
                    watched.y(),
                    watched.z(),
                    pos.0,
                    pos.1,
                    describe_state(i32::from(state.0))
                );
            }
        }
    }

    pub(super) fn begin() {
        INDEX.with(|index| index.set(0));
    }

    /// Hash after one structure piece of the center chunk's pass (a sub-row of the
    /// following structure row; the running index is not advanced).
    pub(super) fn record_piece(
        region: &WorldGenRegion<'_>,
        structure: &Identifier,
        piece_ordinal: usize,
        piece_type: &Identifier,
    ) {
        let center = region.center();
        let pos = (center.0.x, center.0.y);
        if !TRACE_CHUNKS.contains(&pos) {
            return;
        }
        let Some(chunk) = region.try_chunk(pos.0, pos.1, ChunkStatus::StructureStarts) else {
            return;
        };
        let index = INDEX.with(Cell::get);
        let digest = compute_block_hash(chunk.sections());
        eprintln!(
            "FEATURE_TRACE_PIECE {},{},{index},{structure},{piece_ordinal},{piece_type},{digest:x}",
            pos.0, pos.1
        );
        dump_chunk(
            &chunk,
            &format!("piece_{}_{}_{index}_{piece_ordinal}", pos.0, pos.1),
        );
    }

    /// With `STEEL_FEATURE_TRACE_DUMP_DIR` set, writes the chunk's block ids
    /// (section, y, z, x order, big-endian i32) to `<dir>/<name>.bin`.
    fn dump_chunk(chunk: &WorldGenChunkRef<'_>, name: &str) {
        let Ok(dir) = env::var("STEEL_FEATURE_TRACE_DUMP_DIR") else {
            return;
        };
        let mut ids = Vec::with_capacity(16 * 16 * 384 * 4);
        for section in &chunk.sections().sections {
            let section = section.read();
            for y in 0..16 {
                for z in 0..16 {
                    for x in 0..16 {
                        ids.extend_from_slice(
                            &i32::from(section.states().get(x, y, z).0).to_be_bytes(),
                        );
                    }
                }
            }
        }
        let path = format!("{dir}/{name}.bin");
        if let Err(error) = fs::write(&path, ids) {
            eprintln!("FEATURE_TRACE dump failed for {path}: {error}");
        }
    }

    pub(super) fn record(region: &WorldGenRegion<'_>, kind: &str, key: &Identifier) {
        let center = region.center();
        let pos = (center.0.x, center.0.y);
        let index = INDEX.with(|index| {
            let value = index.get();
            index.set(value + 1);
            value
        });
        report_watched(region, pos, index, kind, key);
        report_watched_heights(region, pos, index, kind, key);
        if !TRACE_CHUNKS.contains(&pos) {
            return;
        }
        let Some(chunk) = region.try_chunk(pos.0, pos.1, ChunkStatus::StructureStarts) else {
            return;
        };
        let digest = compute_block_hash(chunk.sections());
        eprintln!(
            "FEATURE_TRACE {},{},{index},{kind} {key},{digest:x}",
            pos.0, pos.1
        );
        dump_chunk(&chunk, &format!("feature_{}_{}_{index}", pos.0, pos.1));
    }
}
