//! POI set for a single chunk section.

use rustc_hash::{FxHashMap, FxHashSet};

use super::poi_instance::PointOfInterest;
use super::poi_storage::OccupationStatus;

/// Stores all POIs within a single chunk section (16x16x16).
///
/// Uses a packed local position `(y << 8 | z << 4 | x)` as the key,
/// with a secondary index by POI type for type-filtered queries.
pub struct PointOfInterestSet {
    pois_by_pos: FxHashMap<u16, PointOfInterest>,
    pois_by_type: FxHashMap<usize, FxHashSet<u16>>,
    dirty: bool,
}

impl Default for PointOfInterestSet {
    fn default() -> Self {
        Self::new()
    }
}

impl PointOfInterestSet {
    /// Creates an empty POI set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pois_by_pos: FxHashMap::default(),
            pois_by_type: FxHashMap::default(),
            dirty: false,
        }
    }

    /// Packs section-local coordinates into a single `u16` key.
    #[inline]
    #[must_use]
    pub const fn pack_local_pos(x: u8, y: u8, z: u8) -> u16 {
        (y as u16) << 8 | (z as u16) << 4 | (x as u16)
    }

    /// Unpacks a `u16` key back into `(x, y, z)` local coordinates.
    #[inline]
    #[must_use]
    pub const fn unpack_local_pos(packed: u16) -> (u8, u8, u8) {
        let x = (packed & 0xF) as u8;
        let z = ((packed >> 4) & 0xF) as u8;
        let y = ((packed >> 8) & 0xF) as u8;
        (x, y, z)
    }

    /// Returns a reference to the inserted POI, or `None` if one already exists at that position.
    pub fn add(&mut self, packed_pos: u16, poi: PointOfInterest) -> Option<&PointOfInterest> {
        if self.pois_by_pos.contains_key(&packed_pos) {
            return None;
        }

        let type_id = poi.poi_type_id;
        self.pois_by_pos.insert(packed_pos, poi);
        self.pois_by_type
            .entry(type_id)
            .or_default()
            .insert(packed_pos);
        self.dirty = true;

        self.pois_by_pos.get(&packed_pos)
    }

    /// Removes and returns the POI at the given packed position, if present.
    pub fn remove(&mut self, packed_pos: u16) -> Option<PointOfInterest> {
        let poi = self.pois_by_pos.remove(&packed_pos)?;
        if let Some(positions) = self.pois_by_type.get_mut(&poi.poi_type_id) {
            positions.remove(&packed_pos);
            if positions.is_empty() {
                self.pois_by_type.remove(&poi.poi_type_id);
            }
        }
        self.dirty = true;
        Some(poi)
    }

    /// Returns a reference to the POI at the given packed position.
    #[must_use]
    pub fn get(&self, packed_pos: u16) -> Option<&PointOfInterest> {
        self.pois_by_pos.get(&packed_pos)
    }

    /// Returns a mutable reference to the POI at the given packed position.
    pub fn get_mut(&mut self, packed_pos: u16) -> Option<&mut PointOfInterest> {
        self.pois_by_pos.get_mut(&packed_pos)
    }

    /// Returns all POIs of the given type matching the occupation status.
    #[must_use]
    pub fn get_by_type(
        &self,
        type_id: usize,
        status: OccupationStatus,
        max_tickets: u32,
    ) -> Vec<&PointOfInterest> {
        let Some(positions) = self.pois_by_type.get(&type_id) else {
            return Vec::new();
        };

        positions
            .iter()
            .filter_map(|pos| self.pois_by_pos.get(pos))
            .filter(|poi| status.matches(poi, max_tickets))
            .collect()
    }

    /// Returns all POIs matching the type predicate and occupation status.
    pub fn get_matching(
        &self,
        type_predicate: &impl Fn(usize) -> bool,
        status: OccupationStatus,
        max_tickets_fn: &impl Fn(usize) -> u32,
    ) -> Vec<&PointOfInterest> {
        self.pois_by_pos
            .values()
            .filter(|poi| {
                type_predicate(poi.poi_type_id)
                    && status.matches(poi, max_tickets_fn(poi.poi_type_id))
            })
            .collect()
    }

    /// Returns `true` if any POIs have been added or removed since last cleared.
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Clears the dirty flag.
    pub const fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// Returns `true` if this set contains no POIs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pois_by_pos.is_empty()
    }

    /// Returns the number of POIs in this set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pois_by_pos.len()
    }

    /// Iterates over all POIs in this set as `(packed_pos, poi)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (u16, &PointOfInterest)> {
        self.pois_by_pos.iter().map(|(&pos, poi)| (pos, poi))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_utils::BlockPos;

    #[test]
    fn test_pack_unpack() {
        for x in 0..16u8 {
            for y in 0..16u8 {
                for z in 0..16u8 {
                    let packed = PointOfInterestSet::pack_local_pos(x, y, z);
                    let (ux, uy, uz) = PointOfInterestSet::unpack_local_pos(packed);
                    assert_eq!((x, y, z), (ux, uy, uz));
                }
            }
        }
    }

    #[test]
    fn test_add_remove_poi() {
        let mut set = PointOfInterestSet::new();
        let packed = PointOfInterestSet::pack_local_pos(5, 10, 3);
        let poi = PointOfInterest::new(BlockPos::new(5, 10, 3), 0, 1);

        assert!(set.add(packed, poi.clone()).is_some());
        assert!(set.add(packed, poi).is_none());

        assert_eq!(set.len(), 1);
        assert!(set.get(packed).is_some());

        let removed = set.remove(packed);
        assert!(removed.is_some());
        assert!(set.is_empty());
    }

    #[test]
    fn test_get_by_type_and_occupation() {
        let mut set = PointOfInterestSet::new();

        let p1 = PointOfInterestSet::pack_local_pos(0, 0, 0);
        set.add(p1, PointOfInterest::new(BlockPos::new(0, 0, 0), 0, 1));

        let p2 = PointOfInterestSet::pack_local_pos(1, 0, 0);
        set.add(p2, PointOfInterest::new(BlockPos::new(1, 0, 0), 0, 1));

        let p3 = PointOfInterestSet::pack_local_pos(2, 0, 0);
        set.add(p3, PointOfInterest::new(BlockPos::new(2, 0, 0), 1, 1));

        assert_eq!(set.get_by_type(0, OccupationStatus::Any, 1).len(), 2);
        assert_eq!(set.get_by_type(1, OccupationStatus::Any, 1).len(), 1);
        assert_eq!(set.get_by_type(0, OccupationStatus::Free, 1).len(), 2);

        set.get_mut(p1).expect("p1 was just added").reserve_ticket();
        assert_eq!(set.get_by_type(0, OccupationStatus::Free, 1).len(), 1);
        assert_eq!(set.get_by_type(0, OccupationStatus::Occupied, 1).len(), 1);
    }
}
