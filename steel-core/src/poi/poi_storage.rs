//! World-level POI storage manager.

use rustc_hash::FxHashMap;
use steel_registry::REGISTRY;
use steel_utils::locks::SyncRwLock;
use steel_utils::{BlockPos, BlockStateId, SectionPos};

use super::poi_instance::PointOfInterest;
use super::poi_set::PointOfInterestSet;
use crate::chunk::section::ChunkSection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OccupationStatus {
    Free,
    Occupied,
    Any,
}

impl OccupationStatus {
    #[must_use]
    pub fn matches(&self, poi: &PointOfInterest, max_tickets: u32) -> bool {
        match self {
            Self::Any => true,
            Self::Free => poi.has_space(),
            Self::Occupied => poi.is_occupied(max_tickets),
        }
    }
}

pub struct PointOfInterestStorage {
    sections: FxHashMap<SectionPos, SyncRwLock<PointOfInterestSet>>,
}

impl Default for PointOfInterestStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[inline]
fn resolve_pos(pos: BlockPos) -> (SectionPos, u16) {
    let section_pos = SectionPos::from_block_pos(pos);
    let packed = PointOfInterestSet::pack_local_pos(
        (pos.0.x & 15) as u8,
        (pos.0.y & 15) as u8,
        (pos.0.z & 15) as u8,
    );
    (section_pos, packed)
}

fn max_tickets_for(type_id: usize) -> u32 {
    REGISTRY
        .poi_types
        .by_id(type_id)
        .map_or(0, |t| t.ticket_count)
}

fn distance_sq(a: BlockPos, b: BlockPos) -> i64 {
    let dx = (a.0.x - b.0.x) as i64;
    let dy = (a.0.y - b.0.y) as i64;
    let dz = (a.0.z - b.0.z) as i64;
    dx * dx + dy * dy + dz * dz
}

impl PointOfInterestStorage {
    #[must_use]
    pub fn new() -> Self {
        Self {
            sections: FxHashMap::default(),
        }
    }

    fn get_or_create_section(
        &mut self,
        section_pos: SectionPos,
    ) -> &SyncRwLock<PointOfInterestSet> {
        self.sections
            .entry(section_pos)
            .or_insert_with(|| SyncRwLock::new(PointOfInterestSet::new()))
    }

    pub fn add(&mut self, pos: BlockPos, poi_type_id: usize, max_tickets: u32) {
        let (section_pos, packed) = resolve_pos(pos);
        let section = self.get_or_create_section(section_pos);
        section
            .write()
            .add(packed, PointOfInterest::new(pos, poi_type_id, max_tickets));
    }

    pub fn remove(&mut self, pos: BlockPos) {
        let (section_pos, packed) = resolve_pos(pos);
        let Some(section) = self.sections.get(&section_pos) else {
            return;
        };

        let mut guard = section.write();
        guard.remove(packed);
        if guard.is_empty() {
            drop(guard);
            self.sections.remove(&section_pos);
        }
    }

    #[must_use]
    pub fn get_type(&self, pos: BlockPos) -> Option<usize> {
        let (section_pos, packed) = resolve_pos(pos);
        self.sections
            .get(&section_pos)?
            .read()
            .get(packed)
            .map(|poi| poi.poi_type_id)
    }

    #[must_use]
    pub fn is_occupied(&self, pos: BlockPos) -> bool {
        let (section_pos, packed) = resolve_pos(pos);
        let Some(section) = self.sections.get(&section_pos) else {
            return false;
        };
        let guard = section.read();
        let Some(poi) = guard.get(packed) else {
            return false;
        };
        poi.is_occupied(max_tickets_for(poi.poi_type_id))
    }

    fn with_poi_mut(&self, pos: BlockPos, f: impl FnOnce(&mut PointOfInterest) -> bool) -> bool {
        let (section_pos, packed) = resolve_pos(pos);
        let Some(section) = self.sections.get(&section_pos) else {
            return false;
        };
        let mut guard = section.write();
        let Some(poi) = guard.get_mut(packed) else {
            return false;
        };
        f(poi)
    }

    pub fn reserve_ticket(&self, pos: BlockPos) -> bool {
        self.with_poi_mut(pos, |poi| poi.reserve_ticket())
    }

    pub fn release_ticket(&self, pos: BlockPos) -> bool {
        self.with_poi_mut(pos, |poi| poi.release_ticket(max_tickets_for(poi.poi_type_id)))
    }

    pub fn get_in_chunk(
        &self,
        type_predicate: &impl Fn(usize) -> bool,
        chunk_x: i32,
        chunk_z: i32,
        status: OccupationStatus,
    ) -> Vec<(BlockPos, usize)> {
        let mut results = Vec::new();

        for (&section_pos, section) in &self.sections {
            if section_pos.x() != chunk_x || section_pos.z() != chunk_z {
                continue;
            }
            let guard = section.read();
            for poi in guard.get_matching(type_predicate, status, &max_tickets_for) {
                results.push((poi.pos, poi.poi_type_id));
            }
        }

        results
    }

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

        for (&section_pos, section) in &self.sections {
            if section_pos.x() < min_section.x()
                || section_pos.x() > max_section.x()
                || section_pos.y() < min_section.y()
                || section_pos.y() > max_section.y()
                || section_pos.z() < min_section.z()
                || section_pos.z() > max_section.z()
            {
                continue;
            }

            let guard = section.read();
            for poi in guard.get_matching(type_predicate, status, &max_tickets_for) {
                let dx = (poi.pos.0.x - center.0.x).abs();
                let dy = (poi.pos.0.y - center.0.y).abs();
                let dz = (poi.pos.0.z - center.0.z).abs();

                if dx <= radius && dy <= radius && dz <= radius {
                    results.push((poi.pos, poi.poi_type_id));
                }
            }
        }

        results
    }

    pub fn get_in_circle(
        &self,
        type_predicate: &impl Fn(usize) -> bool,
        center: BlockPos,
        radius: i32,
        status: OccupationStatus,
    ) -> Vec<(BlockPos, usize)> {
        let radius_sq = (radius as i64) * (radius as i64);
        self.get_in_square(type_predicate, center, radius, status)
            .into_iter()
            .filter(|(pos, _)| distance_sq(*pos, center) <= radius_sq)
            .collect()
    }

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

    #[must_use]
    pub fn count(
        &self,
        type_predicate: &impl Fn(usize) -> bool,
        pos: BlockPos,
        radius: i32,
        status: OccupationStatus,
    ) -> usize {
        self.get_in_circle(type_predicate, pos, radius, status)
            .len()
    }

    pub fn scan_and_populate(&mut self, section: &ChunkSection, section_pos: SectionPos) {
        let registry = &REGISTRY.poi_types;
        let section_lock = self.get_or_create_section(section_pos);
        let mut set = section_lock.write();

        for y in 0..16u8 {
            for z in 0..16u8 {
                for x in 0..16u8 {
                    let state_id = section.states.get(x as usize, y as usize, z as usize);

                    let Some(poi_type_id) = registry.type_id_for_state(state_id) else {
                        continue;
                    };
                    let poi_type = registry.by_id(poi_type_id).unwrap();
                    let block_pos = BlockPos::new(
                        (section_pos.x() << 4) + x as i32,
                        (section_pos.y() << 4) + y as i32,
                        (section_pos.z() << 4) + z as i32,
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
            let poi_type = registry.by_id(type_id).unwrap();
            self.add(pos, type_id, poi_type.ticket_count);
        }
    }

    pub fn remove_section(&mut self, section_pos: SectionPos) {
        self.sections.remove(&section_pos);
    }
}
