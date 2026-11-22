//! World-level POI storage, organized by chunk column and section for spatial
//! queries and per-column persistence.

use std::mem;

use rustc_hash::{FxHashMap, FxHashSet};
use steel_registry::{REGISTRY, RegistryExt};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, PackedSectionBlockPos, SectionPos};

use super::persistence::{PersistentPoiColumn, PersistentPoiRecord};
use super::poi_instance::PointOfInterest;
use super::poi_set::PointOfInterestSet;
use crate::chunk::section::ChunkSection;

/// Filter for POI queries based on ticket availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OccupationStatus {
    /// Only POIs with at least one free ticket.
    Free,
    /// Only POIs with zero free tickets.
    Occupied,
    /// All POIs regardless of ticket status.
    Any,
}

impl OccupationStatus {
    /// Returns `true` if the given POI matches this status filter.
    ///
    /// `max_tickets` is only evaluated for [`Self::Occupied`].
    #[must_use]
    pub fn matches(self, poi: &PointOfInterest, max_tickets: impl FnOnce() -> u32) -> bool {
        match self {
            Self::Any => true,
            Self::Free => poi.has_space(),
            Self::Occupied => poi.is_occupied(max_tickets()),
        }
    }
}

/// Column of POI sets indexed by section Y coordinate.
type PoiColumn = FxHashMap<i32, PointOfInterestSet>;

/// World-level storage for all points of interest.
///
/// Organized as `ChunkPos -> section_y -> PointOfInterestSet`.
pub struct PointOfInterestStorage {
    columns: FxHashMap<ChunkPos, PoiColumn>,
    /// Columns whose authoritative state is held in memory, even when empty.
    loaded: FxHashSet<ChunkPos>,
    /// Loaded columns with changes that have not been persisted yet.
    dirty: FxHashSet<ChunkPos>,
    /// Columns checked on disk that had no persisted data.
    store_missing: FxHashSet<ChunkPos>,
}

impl Default for PointOfInterestStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[inline]
const fn resolve_pos(pos: BlockPos) -> (ChunkPos, i32, PackedSectionBlockPos) {
    let section_pos = SectionPos::from_block_pos(pos);
    let chunk_pos = ChunkPos::new(section_pos.x(), section_pos.z());
    let packed = PackedSectionBlockPos::from_block_pos(pos);
    (chunk_pos, section_pos.y(), packed)
}

fn max_tickets_for(type_id: usize) -> u32 {
    REGISTRY
        .poi_types
        .by_id(type_id)
        .map_or(0, |t| t.ticket_count)
}

pub(crate) fn distance_sq(a: BlockPos, b: BlockPos) -> i64 {
    let dx = i64::from(a.0.x) - i64::from(b.0.x);
    let dy = i64::from(a.0.y) - i64::from(b.0.y);
    let dz = i64::from(a.0.z) - i64::from(b.0.z);
    dx * dx + dy * dy + dz * dz
}

impl PointOfInterestStorage {
    /// Creates an empty POI storage.
    #[must_use]
    pub fn new() -> Self {
        Self {
            columns: FxHashMap::default(),
            loaded: FxHashSet::default(),
            dirty: FxHashSet::default(),
            store_missing: FxHashSet::default(),
        }
    }

    /// Returns `true` if this column's authoritative POI state is in memory.
    #[must_use]
    pub fn is_column_loaded(&self, chunk_pos: ChunkPos) -> bool {
        self.loaded.contains(&chunk_pos)
    }

    /// Returns `true` if this column was read from disk and had no persisted data.
    #[must_use]
    pub fn is_store_missing(&self, chunk_pos: ChunkPos) -> bool {
        self.store_missing.contains(&chunk_pos)
    }

    /// Records that a column has no persisted data.
    pub fn mark_store_missing(&mut self, chunk_pos: ChunkPos) {
        self.store_missing.insert(chunk_pos);
    }

    /// Marks a loaded column as modified since its last save.
    pub fn mark_column_dirty(&mut self, chunk_pos: ChunkPos) {
        self.loaded.insert(chunk_pos);
        self.dirty.insert(chunk_pos);
        self.store_missing.remove(&chunk_pos);
    }

    /// Replaces a column with its persisted representation.
    ///
    /// `dirty` keeps the column queued for a rewrite, for example after a
    /// failed unload save.
    pub fn insert_loaded_column(
        &mut self,
        chunk_pos: ChunkPos,
        column: &PersistentPoiColumn,
        dirty: bool,
    ) {
        self.columns.remove(&chunk_pos);
        for record in &column.records {
            let Some(type_id) = REGISTRY.poi_types.id_from_key(&record.poi_type) else {
                tracing::warn!(
                    poi_type = %record.poi_type,
                    chunk = ?chunk_pos,
                    "Skipping persisted POI with an unknown type"
                );
                continue;
            };
            let pos = BlockPos::new(
                chunk_pos.0.x * 16 + i32::from(record.x),
                i32::from(record.y),
                chunk_pos.0.y * 16 + i32::from(record.z),
            );
            let (_, section_y, packed) = resolve_pos(pos);
            self.columns
                .entry(chunk_pos)
                .or_default()
                .entry(section_y)
                .or_default()
                .add(
                    packed,
                    PointOfInterest {
                        pos,
                        poi_type_id: type_id,
                        free_tickets: record.free_tickets,
                    },
                );
        }

        self.loaded.insert(chunk_pos);
        self.store_missing.remove(&chunk_pos);
        if dirty {
            self.dirty.insert(chunk_pos);
        } else {
            self.dirty.remove(&chunk_pos);
        }
    }

    /// Starts a new authoritative column, discarding any previous in-memory state.
    pub fn start_column(&mut self, chunk_pos: ChunkPos) {
        self.columns.remove(&chunk_pos);
        self.loaded.insert(chunk_pos);
        self.dirty.remove(&chunk_pos);
        self.store_missing.remove(&chunk_pos);
    }

    /// Returns the persisted representation of a column.
    #[must_use]
    pub fn collect_column(&self, chunk_pos: ChunkPos) -> PersistentPoiColumn {
        let mut records = Vec::new();
        if let Some(column) = self.columns.get(&chunk_pos) {
            for set in column.values() {
                for (_, poi) in set.iter() {
                    let Some(poi_type) = REGISTRY.poi_types.by_id(poi.poi_type_id) else {
                        continue;
                    };
                    records.push(PersistentPoiRecord {
                        x: (poi.pos.0.x - chunk_pos.0.x * 16) as u8,
                        y: poi.pos.0.y as i16,
                        z: (poi.pos.0.z - chunk_pos.0.y * 16) as u8,
                        poi_type: poi_type.key.clone(),
                        free_tickets: poi.free_tickets,
                    });
                }
            }
        }
        PersistentPoiColumn { records }
    }

    /// Returns a snapshot of a dirty column and clears its dirty flag.
    #[must_use]
    pub fn dirty_column_snapshot(&mut self, chunk_pos: ChunkPos) -> Option<PersistentPoiColumn> {
        if !self.dirty.remove(&chunk_pos) {
            return None;
        }
        Some(self.collect_column(chunk_pos))
    }

    /// Drains every dirty column snapshot, keeping columns resident.
    #[must_use]
    pub fn take_dirty_snapshots(&mut self) -> Vec<(ChunkPos, PersistentPoiColumn)> {
        let dirty = mem::take(&mut self.dirty);
        dirty
            .into_iter()
            .map(|chunk_pos| (chunk_pos, self.collect_column(chunk_pos)))
            .collect()
    }

    fn get_or_create_set(
        &mut self,
        chunk_pos: ChunkPos,
        section_y: i32,
    ) -> &mut PointOfInterestSet {
        self.columns
            .entry(chunk_pos)
            .or_default()
            .entry(section_y)
            .or_default()
    }

    /// Adds a POI at the given block position.
    pub fn add(&mut self, pos: BlockPos, poi_type_id: usize, max_tickets: u32) {
        let (chunk_pos, section_y, packed) = resolve_pos(pos);
        {
            let set = self.get_or_create_set(chunk_pos, section_y);
            set.add(packed, PointOfInterest::new(pos, poi_type_id, max_tickets));
        }
        self.mark_column_dirty(chunk_pos);
    }

    /// Removes the POI at the given block position.
    pub fn remove(&mut self, pos: BlockPos) {
        let (chunk_pos, section_y, packed) = resolve_pos(pos);
        let Some(column) = self.columns.get_mut(&chunk_pos) else {
            return;
        };
        let Some(set) = column.get_mut(&section_y) else {
            return;
        };

        if set.remove(packed).is_none() {
            return;
        }
        if set.is_empty() {
            column.remove(&section_y);
            if column.is_empty() {
                self.columns.remove(&chunk_pos);
            }
        }
        self.mark_column_dirty(chunk_pos);
    }

    /// Returns the POI type ID at the given position, if any.
    #[must_use]
    pub fn get_type(&self, pos: BlockPos) -> Option<usize> {
        let (chunk_pos, section_y, packed) = resolve_pos(pos);
        self.columns
            .get(&chunk_pos)?
            .get(&section_y)?
            .get(packed)
            .map(|poi| poi.poi_type_id)
    }

    /// Returns `true` if the POI at the given position has all tickets reserved.
    #[must_use]
    pub fn is_occupied(&self, pos: BlockPos) -> bool {
        let (chunk_pos, section_y, packed) = resolve_pos(pos);
        let Some(column) = self.columns.get(&chunk_pos) else {
            return false;
        };
        let Some(set) = column.get(&section_y) else {
            return false;
        };
        let Some(poi) = set.get(packed) else {
            return false;
        };
        poi.is_occupied(max_tickets_for(poi.poi_type_id))
    }

    /// Reserves a ticket at the given position. Returns `true` if successful.
    #[must_use]
    pub fn reserve_ticket(&mut self, pos: BlockPos) -> bool {
        let (chunk_pos, section_y, packed) = resolve_pos(pos);
        let Some(poi) = self
            .columns
            .get_mut(&chunk_pos)
            .and_then(|c| c.get_mut(&section_y))
            .and_then(|set| set.get_mut(packed))
        else {
            return false;
        };
        if !poi.reserve_ticket() {
            return false;
        }
        self.mark_column_dirty(chunk_pos);
        true
    }

    /// Releases a ticket at the given position. Returns `true` if successful.
    #[must_use]
    pub fn release_ticket(&mut self, pos: BlockPos) -> bool {
        let (chunk_pos, section_y, packed) = resolve_pos(pos);
        let Some(poi) = self
            .columns
            .get_mut(&chunk_pos)
            .and_then(|c| c.get_mut(&section_y))
            .and_then(|set| set.get_mut(packed))
        else {
            return false;
        };
        if !poi.release_ticket(max_tickets_for(poi.poi_type_id)) {
            return false;
        }
        self.mark_column_dirty(chunk_pos);
        true
    }

    /// Returns all matching POIs in a specific chunk column.
    #[must_use]
    pub fn get_in_chunk(
        &self,
        type_predicate: &impl Fn(usize) -> bool,
        chunk_x: i32,
        chunk_z: i32,
        status: OccupationStatus,
    ) -> Vec<(BlockPos, usize)> {
        let chunk_pos = ChunkPos::new(chunk_x, chunk_z);
        let Some(column) = self.columns.get(&chunk_pos) else {
            return Vec::new();
        };

        let mut results = Vec::new();
        for set in column.values() {
            for (_, poi) in set.iter() {
                if type_predicate(poi.poi_type_id)
                    && status.matches(poi, || max_tickets_for(poi.poi_type_id))
                {
                    results.push((poi.pos, poi.poi_type_id));
                }
            }
        }
        results
    }

    /// Returns all matching POIs within a cubic region centered on `center`.
    #[must_use]
    pub fn get_in_square(
        &self,
        type_predicate: &impl Fn(usize) -> bool,
        center: BlockPos,
        radius: i32,
        status: OccupationStatus,
    ) -> Vec<(BlockPos, usize)> {
        let min_section = SectionPos::from_block_pos(BlockPos::new(
            center.0.x - radius,
            center.0.y - radius,
            center.0.z - radius,
        ));
        let max_section = SectionPos::from_block_pos(BlockPos::new(
            center.0.x + radius,
            center.0.y + radius,
            center.0.z + radius,
        ));

        let mut results = Vec::new();

        for cx in min_section.x()..=max_section.x() {
            for cz in min_section.z()..=max_section.z() {
                let chunk_pos = ChunkPos::new(cx, cz);
                let Some(column) = self.columns.get(&chunk_pos) else {
                    continue;
                };

                for section_y in min_section.y()..=max_section.y() {
                    let Some(set) = column.get(&section_y) else {
                        continue;
                    };

                    for poi in set.get_matching(type_predicate, status, &max_tickets_for) {
                        let dx = (poi.pos.0.x - center.0.x).abs();
                        let dy = (poi.pos.0.y - center.0.y).abs();
                        let dz = (poi.pos.0.z - center.0.z).abs();

                        if dx <= radius && dy <= radius && dz <= radius {
                            results.push((poi.pos, poi.poi_type_id));
                        }
                    }
                }
            }
        }

        results
    }

    /// Returns all matching POIs within a vanilla horizontal square centered on `center`.
    ///
    /// Mirrors `PoiManager.getInSquare`: X/Z are constrained by `radius`, while Y is not.
    #[must_use]
    pub fn get_in_horizontal_square(
        &self,
        type_predicate: &impl Fn(usize) -> bool,
        center: BlockPos,
        radius: i32,
        status: OccupationStatus,
    ) -> Vec<(BlockPos, usize)> {
        let center_chunk = ChunkPos::from_block_pos(center);
        let chunk_radius = radius.div_euclid(16) + 1;
        let mut results = Vec::new();

        for cx in center_chunk.0.x - chunk_radius..=center_chunk.0.x + chunk_radius {
            for cz in center_chunk.0.y - chunk_radius..=center_chunk.0.y + chunk_radius {
                let chunk_pos = ChunkPos::new(cx, cz);
                let Some(column) = self.columns.get(&chunk_pos) else {
                    continue;
                };

                for set in column.values() {
                    for poi in set.get_matching(type_predicate, status, &max_tickets_for) {
                        let dx = (poi.pos.0.x - center.0.x).abs();
                        let dz = (poi.pos.0.z - center.0.z).abs();
                        if dx <= radius && dz <= radius {
                            results.push((poi.pos, poi.poi_type_id));
                        }
                    }
                }
            }
        }

        results
    }

    /// Counts matching POIs within a spherical region.
    #[must_use]
    pub fn count(
        &self,
        type_predicate: &impl Fn(usize) -> bool,
        pos: BlockPos,
        radius: i32,
        status: OccupationStatus,
    ) -> usize {
        let radius_sq = i64::from(radius) * i64::from(radius);
        self.count_in_square(type_predicate, pos, radius, status, &|candidate| {
            distance_sq(candidate, pos) <= radius_sq
        })
    }

    /// Counts matching POIs within a cubic region, filtered by an additional predicate.
    fn count_in_square(
        &self,
        type_predicate: &impl Fn(usize) -> bool,
        center: BlockPos,
        radius: i32,
        status: OccupationStatus,
        filter: &impl Fn(BlockPos) -> bool,
    ) -> usize {
        let min_section = SectionPos::from_block_pos(BlockPos::new(
            center.0.x - radius,
            center.0.y - radius,
            center.0.z - radius,
        ));
        let max_section = SectionPos::from_block_pos(BlockPos::new(
            center.0.x + radius,
            center.0.y + radius,
            center.0.z + radius,
        ));

        let mut count = 0;

        for cx in min_section.x()..=max_section.x() {
            for cz in min_section.z()..=max_section.z() {
                let chunk_pos = ChunkPos::new(cx, cz);
                let Some(column) = self.columns.get(&chunk_pos) else {
                    continue;
                };

                for section_y in min_section.y()..=max_section.y() {
                    let Some(set) = column.get(&section_y) else {
                        continue;
                    };

                    for poi in set.get_matching(type_predicate, status, &max_tickets_for) {
                        let dx = (poi.pos.0.x - center.0.x).abs();
                        let dy = (poi.pos.0.y - center.0.y).abs();
                        let dz = (poi.pos.0.z - center.0.z).abs();

                        if dx <= radius && dy <= radius && dz <= radius && filter(poi.pos) {
                            count += 1;
                        }
                    }
                }
            }
        }

        count
    }

    /// Scans a chunk section for POI block states and populates the storage.
    ///
    /// # Panics
    /// Panics if the POI type registry contains an inconsistent state-to-type mapping.
    pub fn scan_and_populate(&mut self, section: &ChunkSection, section_pos: SectionPos) {
        let registry = &REGISTRY.poi_types;
        let chunk_pos = ChunkPos::new(section_pos.x(), section_pos.z());
        {
            let set = self.get_or_create_set(chunk_pos, section_pos.y());

            for y in 0..16u8 {
                for z in 0..16u8 {
                    for x in 0..16u8 {
                        let state_id = section.states().get(x as usize, y as usize, z as usize);

                        let Some(poi_type_id) = registry.type_id_for_state(state_id) else {
                            continue;
                        };
                        let poi_type = registry
                            .by_id(poi_type_id)
                            .expect("POI type ID from state lookup must be valid");
                        let block_pos = BlockPos::new(
                            (section_pos.x() << 4) + i32::from(x),
                            (section_pos.y() << 4) + i32::from(y),
                            (section_pos.z() << 4) + i32::from(z),
                        );
                        let packed = PackedSectionBlockPos::from_block_pos(block_pos);
                        set.add(
                            packed,
                            PointOfInterest::new(block_pos, poi_type_id, poi_type.ticket_count),
                        );
                    }
                }
            }
        }
        self.mark_column_dirty(chunk_pos);
    }

    /// Updates POI storage when a block state changes.
    ///
    /// # Panics
    /// Panics if the POI type registry contains an inconsistent state-to-type mapping.
    pub fn on_block_state_change(
        &mut self,
        pos: BlockPos,
        old_state: BlockStateId,
        new_state: BlockStateId,
    ) {
        let registry = &REGISTRY.poi_types;
        let old_poi = registry.type_id_for_state(old_state);
        let new_poi = registry.type_id_for_state(new_state);

        if old_poi == new_poi {
            return;
        }

        if old_poi.is_some() {
            self.remove(pos);
        }

        if let Some(type_id) = new_poi {
            let poi_type = registry
                .by_id(type_id)
                .expect("POI type ID from state lookup must be valid");
            self.add(pos, type_id, poi_type.ticket_count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OccupationStatus, PointOfInterestStorage};
    use steel_registry::{REGISTRY, RegistryExt, init_vanilla_registry, vanilla_poi_types};
    use steel_utils::{BlockPos, ChunkPos};

    fn sorted_positions(mut positions: Vec<(BlockPos, usize)>) -> Vec<BlockPos> {
        positions.sort_by_key(|(pos, _)| (pos.x(), pos.y(), pos.z()));
        positions.into_iter().map(|(pos, _)| pos).collect()
    }

    #[test]
    fn column_snapshot_roundtrips_through_persistent_form() {
        init_vanilla_registry();
        let chunk_pos = ChunkPos::new(-2, 3);
        let pos = BlockPos::new(-2 * 16 + 5, 64, 3 * 16 + 9);
        let poi_type = &vanilla_poi_types::HOME;
        let type_id = REGISTRY
            .poi_types
            .id_from_key(&poi_type.key)
            .expect("home POI type should be registered");

        let mut storage = PointOfInterestStorage::new();
        storage.add(pos, type_id, poi_type.ticket_count);
        assert!(storage.reserve_ticket(pos));
        let column = storage
            .dirty_column_snapshot(chunk_pos)
            .expect("added column should be dirty");
        assert!(storage.dirty_column_snapshot(chunk_pos).is_none());

        let mut restored = PointOfInterestStorage::new();
        restored.insert_loaded_column(chunk_pos, &column, false);
        assert!(restored.is_column_loaded(chunk_pos));

        let column = restored.collect_column(chunk_pos);
        assert_eq!(column.records.len(), 1);
        let record = &column.records[0];
        assert_eq!(record.x, 5);
        assert_eq!(record.y, 64);
        assert_eq!(record.z, 9);
        assert_eq!(record.poi_type, poi_type.key);
        assert_eq!(record.free_tickets, poi_type.ticket_count - 1);
    }

    #[test]
    fn horizontal_square_query_matches_y_unbounded_vanilla_search() {
        init_vanilla_registry();
        let mut storage = PointOfInterestStorage::new();
        storage.add(BlockPos::new(0, -64, 0), 7, 0);
        storage.add(BlockPos::new(0, 320, 0), 7, 0);
        storage.add(BlockPos::new(2, 64, 0), 7, 0);
        storage.add(BlockPos::new(0, 64, 2), 7, 0);

        let positions = sorted_positions(storage.get_in_horizontal_square(
            &|type_id| type_id == 7,
            BlockPos::new(0, 64, 0),
            1,
            OccupationStatus::Any,
        ));

        assert_eq!(
            positions,
            vec![BlockPos::new(0, -64, 0), BlockPos::new(0, 320, 0)]
        );
    }
}
