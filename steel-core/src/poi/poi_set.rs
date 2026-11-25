//! POIs inside one 16×16×16 chunk section.

use rustc_hash::{FxHashMap, FxHashSet};
use steel_registry::poi::PoiTypeRef;
use steel_utils::PackedSectionBlockPos;

use super::poi_instance::PointOfInterest;
use super::poi_storage::OccupationStatus;

/// POIs in a section, indexed by position and by type.
///
/// Keeping a type index lets filtered queries skip sections with no matching type.
pub struct PointOfInterestSet {
    pois_by_pos: FxHashMap<PackedSectionBlockPos, PointOfInterest>,
    pois_by_type: FxHashMap<PoiTypeRef, FxHashSet<PackedSectionBlockPos>>,
    dirty: bool,
}

impl Default for PointOfInterestSet {
    fn default() -> Self {
        Self::new()
    }
}

impl PointOfInterestSet {
    /// Creates an empty section.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pois_by_pos: FxHashMap::default(),
            pois_by_type: FxHashMap::default(),
            dirty: false,
        }
    }

    /// Inserts a POI. Returns the stored entry, or `None` if the slot was taken.
    pub fn add(
        &mut self,
        packed_pos: PackedSectionBlockPos,
        poi: PointOfInterest,
    ) -> Option<&PointOfInterest> {
        if self.pois_by_pos.contains_key(&packed_pos) {
            return None;
        }

        let poi_type = poi.poi_type;
        self.pois_by_pos.insert(packed_pos, poi);
        self.pois_by_type
            .entry(poi_type)
            .or_default()
            .insert(packed_pos);
        self.dirty = true;

        self.pois_by_pos.get(&packed_pos)
    }

    /// Removes the POI at `packed_pos`, if there is one.
    pub fn remove(&mut self, packed_pos: PackedSectionBlockPos) -> Option<PointOfInterest> {
        let poi = self.pois_by_pos.remove(&packed_pos)?;
        if let Some(positions) = self.pois_by_type.get_mut(&poi.poi_type) {
            positions.remove(&packed_pos);
            if positions.is_empty() {
                self.pois_by_type.remove(&poi.poi_type);
            }
        }
        self.dirty = true;
        Some(poi)
    }

    /// The POI at `packed_pos`, if present.
    #[must_use]
    pub fn get(&self, packed_pos: PackedSectionBlockPos) -> Option<&PointOfInterest> {
        self.pois_by_pos.get(&packed_pos)
    }

    /// Mutable access to the POI at `packed_pos`, if present.
    pub fn get_mut(&mut self, packed_pos: PackedSectionBlockPos) -> Option<&mut PointOfInterest> {
        self.pois_by_pos.get_mut(&packed_pos)
    }

    /// POIs of `poi_type` whose ticket state matches `status`.
    #[must_use]
    pub fn get_by_type(
        &self,
        poi_type: PoiTypeRef,
        status: OccupationStatus,
    ) -> Vec<&PointOfInterest> {
        let Some(positions) = self.pois_by_type.get(&poi_type) else {
            return Vec::new();
        };

        positions
            .iter()
            .filter_map(|pos| self.pois_by_pos.get(pos))
            .filter(|poi| status.matches(poi))
            .collect()
    }

    /// POIs passing both the type predicate and the ticket filter.
    pub fn get_matching(
        &self,
        type_predicate: &impl Fn(PoiTypeRef) -> bool,
        status: OccupationStatus,
    ) -> Vec<&PointOfInterest> {
        self.pois_by_pos
            .values()
            .filter(|poi| type_predicate(poi.poi_type) && status.matches(poi))
            .collect()
    }

    /// Whether this section has had POIs added or removed since the flag was cleared.
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Marks this section as clean.
    pub const fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// Whether the section holds no POIs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pois_by_pos.is_empty()
    }

    /// Number of POIs in the section.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pois_by_pos.len()
    }

    /// Walks the section as `(packed position, POI)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (PackedSectionBlockPos, &PointOfInterest)> {
        self.pois_by_pos.iter().map(|(&pos, poi)| (pos, poi))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::{init_vanilla_registry, vanilla_poi_types};
    use steel_utils::BlockPos;

    fn packed(x: u8, y: u8, z: u8) -> PackedSectionBlockPos {
        let Some(pos) = PackedSectionBlockPos::from_local_xyz(x, y, z) else {
            panic!("valid local packed section block position was rejected");
        };
        pos
    }

    #[test]
    fn test_pack_unpack() {
        for x in 0..16u8 {
            for y in 0..16u8 {
                for z in 0..16u8 {
                    let pos = packed(x, y, z);
                    assert_eq!((x, y, z), (pos.x(), pos.y(), pos.z()));
                }
            }
        }
    }

    #[test]
    fn test_add_remove_poi() {
        init_vanilla_registry();
        let mut set = PointOfInterestSet::new();
        let packed = packed(5, 10, 3);
        let poi = PointOfInterest::new(BlockPos::new(5, 10, 3), &vanilla_poi_types::HOME);

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
        init_vanilla_registry();
        let home = &vanilla_poi_types::HOME;
        let meeting = &vanilla_poi_types::MEETING;
        let mut set = PointOfInterestSet::new();

        let p1 = packed(0, 0, 0);
        set.add(p1, PointOfInterest::new(BlockPos::new(0, 0, 0), home));

        let p2 = packed(1, 0, 0);
        set.add(p2, PointOfInterest::new(BlockPos::new(1, 0, 0), home));

        let p3 = packed(2, 0, 0);
        set.add(p3, PointOfInterest::new(BlockPos::new(2, 0, 0), meeting));

        assert_eq!(set.get_by_type(home, OccupationStatus::Any).len(), 2);
        assert_eq!(set.get_by_type(meeting, OccupationStatus::Any).len(), 1);
        assert_eq!(set.get_by_type(home, OccupationStatus::Free).len(), 2);

        set.get_mut(p1).expect("p1 was just added").reserve_ticket();
        assert_eq!(set.get_by_type(home, OccupationStatus::Free).len(), 1);
        assert_eq!(set.get_by_type(home, OccupationStatus::Occupied).len(), 1);
    }
}
