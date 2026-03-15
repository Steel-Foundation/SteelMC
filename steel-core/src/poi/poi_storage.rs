//! World-level POI storage manager.
//!
//! Tracks special blocks (beds, workstations, bells, nether portals, etc.)
//! so game systems can efficiently query for nearby points of interest
//! without scanning every block. Organized by chunk column for efficient
//! load/unload and spatial queries.

use rustc_hash::FxHashMap;
use steel_registry::{REGISTRY, RegistryExt};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, SectionPos};

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
    #[must_use]
    pub const fn matches(&self, poi: &PointOfInterest, max_tickets: u32) -> bool {
        match self {
            Self::Any => true,
            Self::Free => poi.has_space(),
            Self::Occupied => poi.is_occupied(max_tickets),
        }
    }
}

/// Column of POI sets indexed by section Y coordinate.
type PoiColumn = FxHashMap<i32, PointOfInterestSet>;

/// World-level storage for all points of interest.
///
/// Organized as a two-level map: `ChunkPos -> section_y -> PointOfInterestSet`.
/// This structure mirrors chunk lifecycle (load/unload per column) and provides
/// efficient spatial queries by narrowing to relevant columns first.
pub struct PointOfInterestStorage {
    columns: FxHashMap<ChunkPos, PoiColumn>,
}

impl Default for PointOfInterestStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[inline]
const fn resolve_pos(pos: BlockPos) -> (ChunkPos, i32, u16) {
    let section_pos = SectionPos::from_block_pos(pos);
    let chunk_pos = ChunkPos::new(section_pos.x(), section_pos.z());
    let packed = PointOfInterestSet::pack_local_pos(
        (pos.0.x & 15) as u8,
        (pos.0.y & 15) as u8,
        (pos.0.z & 15) as u8,
    );
    (chunk_pos, section_pos.y(), packed)
}

fn max_tickets_for(type_id: usize) -> u32 {
    REGISTRY
        .poi_types
        .by_id(type_id)
        .map_or(0, |t| t.ticket_count)
}

fn distance_sq(a: BlockPos, b: BlockPos) -> i64 {
    let dx = i64::from(a.0.x - b.0.x);
    let dy = i64::from(a.0.y - b.0.y);
    let dz = i64::from(a.0.z - b.0.z);
    dx * dx + dy * dy + dz * dz
}

impl PointOfInterestStorage {
    /// Creates an empty POI storage.
    #[must_use]
    pub fn new() -> Self {
        Self {
            columns: FxHashMap::default(),
        }
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
        let set = self.get_or_create_set(chunk_pos, section_y);
        set.add(packed, PointOfInterest::new(pos, poi_type_id, max_tickets));
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

        set.remove(packed);
        if set.is_empty() {
            column.remove(&section_y);
            if column.is_empty() {
                self.columns.remove(&chunk_pos);
            }
        }
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
        let Some(set) = self
            .columns
            .get_mut(&chunk_pos)
            .and_then(|c| c.get_mut(&section_y))
        else {
            return false;
        };
        let Some(poi) = set.get_mut(packed) else {
            return false;
        };
        poi.reserve_ticket()
    }

    /// Releases a ticket at the given position. Returns `true` if successful.
    #[must_use]
    pub fn release_ticket(&mut self, pos: BlockPos) -> bool {
        let (chunk_pos, section_y, packed) = resolve_pos(pos);
        let Some(set) = self
            .columns
            .get_mut(&chunk_pos)
            .and_then(|c| c.get_mut(&section_y))
        else {
            return false;
        };
        let Some(poi) = set.get_mut(packed) else {
            return false;
        };
        poi.release_ticket(max_tickets_for(poi.poi_type_id))
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
            for poi in set.get_matching(type_predicate, status, &max_tickets_for) {
                results.push((poi.pos, poi.poi_type_id));
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

    /// Returns all matching POIs within a spherical region.
    #[must_use]
    pub fn get_in_circle(
        &self,
        type_predicate: &impl Fn(usize) -> bool,
        center: BlockPos,
        radius: i32,
        status: OccupationStatus,
    ) -> Vec<(BlockPos, usize)> {
        let radius_sq = i64::from(radius) * i64::from(radius);
        self.get_in_square(type_predicate, center, radius, status)
            .into_iter()
            .filter(|(pos, _)| distance_sq(*pos, center) <= radius_sq)
            .collect()
    }

    /// Returns the closest matching POI within radius, if any.
    #[must_use]
    pub fn get_nearest(
        &self,
        type_predicate: &impl Fn(usize) -> bool,
        pos: BlockPos,
        radius: i32,
        status: OccupationStatus,
    ) -> Option<(BlockPos, usize)> {
        self.get_in_circle(type_predicate, pos, radius, status)
            .into_iter()
            .min_by_key(|(candidate, _)| distance_sq(*candidate, pos))
    }

    /// Returns all matching POIs within radius, sorted by distance (nearest first).
    #[must_use]
    pub fn get_sorted_by_distance(
        &self,
        type_predicate: &impl Fn(usize) -> bool,
        pos: BlockPos,
        radius: i32,
        status: OccupationStatus,
    ) -> Vec<(BlockPos, usize)> {
        let mut results = self.get_in_circle(type_predicate, pos, radius, status);
        results.sort_by_key(|(candidate, _)| distance_sq(*candidate, pos));
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
        let set = self.get_or_create_set(chunk_pos, section_pos.y());

        for y in 0..16u8 {
            for z in 0..16u8 {
                for x in 0..16u8 {
                    let state_id = section.states.get(x as usize, y as usize, z as usize);

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
                    let packed = PointOfInterestSet::pack_local_pos(x, y, z);
                    set.add(
                        packed,
                        PointOfInterest::new(block_pos, poi_type_id, poi_type.ticket_count),
                    );
                }
            }
        }
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

    /// Collects all POI data in a chunk column for persistence.
    ///
    /// Returns `(BlockPos, free_tickets)` for each POI.
    #[must_use]
    pub fn collect_for_chunk(&self, chunk_pos: ChunkPos) -> Vec<(BlockPos, u32)> {
        let Some(column) = self.columns.get(&chunk_pos) else {
            return Vec::new();
        };
        let mut results = Vec::new();
        for set in column.values() {
            for (_, poi) in set.iter() {
                results.push((poi.pos, poi.free_tickets));
            }
        }
        results
    }

    /// Restores ticket state for POIs after loading from disk.
    ///
    /// Called after `scan_and_populate` has created fresh POIs from block states.
    /// Applies saved `free_tickets` values to matching positions.
    pub fn restore_tickets(&mut self, chunk_pos: ChunkPos, tickets: &[(BlockPos, u32)]) {
        let Some(column) = self.columns.get_mut(&chunk_pos) else {
            return;
        };
        for &(pos, free_tickets) in tickets {
            let section_y = SectionPos::block_to_section_coord(pos.0.y);
            let packed = PointOfInterestSet::pack_local_pos(
                (pos.0.x & 15) as u8,
                (pos.0.y & 15) as u8,
                (pos.0.z & 15) as u8,
            );
            if let Some(set) = column.get_mut(&section_y)
                && let Some(poi) = set.get_mut(packed)
            {
                poi.free_tickets = free_tickets;
            }
        }
    }

    /// Removes all POI data for a chunk column. Called during chunk unload.
    pub fn remove_chunk(&mut self, chunk_pos: ChunkPos) {
        self.columns.remove(&chunk_pos);
    }
}
