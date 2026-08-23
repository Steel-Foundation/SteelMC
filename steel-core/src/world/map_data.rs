//! Per-world filled-map saved data (`MapItemSavedData`).

use rustc_hash::FxHashMap;
use steel_registry::data_components::components::MapId;
use steel_utils::Identifier;

const MAP_SIZE: usize = 128;
const COLOR_LEN: usize = MAP_SIZE * MAP_SIZE;
const MAX_SCALE: u8 = 4;

/// Vanilla `MapItemSavedData`.
#[derive(Clone)]
pub struct MapItemSavedData {
    /// Map center X in world coordinates.
    pub center_x: i32,
    /// Map center Z in world coordinates.
    pub center_z: i32,
    /// Dimension the map belongs to.
    pub dimension: Identifier,
    /// Scale in `0..=4`.
    pub scale: u8,
    /// Packed map colors, 128×128.
    pub colors: [u8; COLOR_LEN],
    /// Whether exploration and further scaling are disabled.
    pub locked: bool,
    /// Whether player markers are shown.
    pub tracking_position: bool,
    /// Whether off-map players still get a marker.
    pub unlimited_tracking: bool,
}

impl MapItemSavedData {
    /// Vanilla `MapItemSavedData.createFresh`.
    #[must_use]
    pub fn create_fresh(
        origin_x: i32,
        origin_z: i32,
        scale: u8,
        tracking_position: bool,
        unlimited_tracking: bool,
        dimension: Identifier,
    ) -> Self {
        let scale = scale.min(MAX_SCALE);
        let size = 128 * (1 << scale);
        let area_x = (i64::from(origin_x) + 64).div_euclid(i64::from(size));
        let area_z = (i64::from(origin_z) + 64).div_euclid(i64::from(size));
        let center_x = (area_x * i64::from(size) + i64::from(size) / 2 - 64) as i32;
        let center_z = (area_z * i64::from(size) + i64::from(size) / 2 - 64) as i32;
        Self {
            center_x,
            center_z,
            dimension,
            scale,
            colors: [0; COLOR_LEN],
            locked: false,
            tracking_position,
            unlimited_tracking,
        }
    }

    /// Vanilla `MapItemSavedData.locked`.
    #[must_use]
    pub fn locked(&self) -> Self {
        let mut copy = self.clone();
        copy.locked = true;
        copy
    }

    /// Vanilla `MapItemSavedData.scaled`.
    #[must_use]
    pub fn scaled(&self) -> Self {
        Self::create_fresh(
            self.center_x,
            self.center_z,
            self.scale.saturating_add(1).min(MAX_SCALE),
            self.tracking_position,
            self.unlimited_tracking,
            self.dimension.clone(),
        )
    }
}

/// World-owned map ID allocator and saved-data table.
#[derive(Default)]
pub struct MapDataStore {
    next_id: i32,
    maps: FxHashMap<i32, MapItemSavedData>,
}

impl MapDataStore {
    /// Allocates a new map id and stores `data`.
    pub fn insert_new(&mut self, data: MapItemSavedData) -> MapId {
        let id = self.next_id;
        self.next_id += 1;
        self.maps.insert(id, data);
        MapId::new(id)
    }

    /// Returns saved data for `id`.
    #[must_use]
    pub fn get(&self, id: MapId) -> Option<&MapItemSavedData> {
        self.maps.get(&id.id())
    }

    /// Returns mutable saved data for `id`.
    pub fn get_mut(&mut self, id: MapId) -> Option<&mut MapItemSavedData> {
        self.maps.get_mut(&id.id())
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::Identifier;

    use super::MapItemSavedData;

    #[test]
    fn create_fresh_snaps_center_to_the_scale_grid() {
        let data = MapItemSavedData::create_fresh(
            100,
            0,
            0,
            true,
            false,
            Identifier::vanilla_static("overworld"),
        );
        assert_eq!(data.center_x, 128);
        assert_eq!(data.center_z, 0);
        assert_eq!(data.scale, 0);
        assert!(!data.locked);
    }
}
