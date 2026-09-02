//! Domain-scoped map saved data.

pub(crate) mod biome_preview;
mod terrain;
mod tracking;

use std::{collections::BTreeMap, io, sync::Arc};

use steel_registry::data_components::MapId;
use steel_utils::{
    BlockPos, Identifier,
    locks::{SyncMutex, SyncRwLock},
    saved_data::{SavedDataManager, names as saved_data_names},
};
use tokio::task::spawn_blocking;
use wincode::{SchemaRead, SchemaWrite};

use crate::{config::ResolvedDomainConfig, server::worlds::WorldMap, world::World};

pub(crate) use tracking::CarriedMap;
use tracking::MapTrackingState;

const MAP_COLOR_COUNT: usize = 128 * 128;

/// Map stores keyed by Steel domain.
pub(crate) struct DomainMapData {
    domains: BTreeMap<String, Arc<MapDataStore>>,
}

/// Vanilla's logical-server-owned map index and map saved data for one domain.
pub(crate) struct MapDataStore {
    saved_data: SavedDataManager,
    state: SyncRwLock<MapDataState>,
    tracking: SyncMutex<MapTrackingState>,
}

struct MapDataState {
    last_map_id: i32,
    maps: BTreeMap<i32, MapItemSavedData>,
    revision: u64,
    saved_revision: u64,
}

/// Persistent data backing one filled map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MapItemSavedData {
    pub(crate) center_x: i32,
    pub(crate) center_z: i32,
    /// Steel loaded-world identity; Vanilla's one-world-per-dimension model does
    /// not require this distinction.
    pub(crate) world: Identifier,
    pub(crate) dimension_type: Identifier,
    pub(crate) tracking_position: bool,
    pub(crate) unlimited_tracking: bool,
    pub(crate) scale: i8,
    pub(crate) colors: Vec<u8>,
    pub(crate) locked: bool,
}

/// Data required to allocate one fresh filled map.
pub(crate) struct NewMapData {
    pub(crate) origin: BlockPos,
    pub(crate) scale: i8,
    pub(crate) tracking_position: bool,
    pub(crate) unlimited_tracking: bool,
    pub(crate) world: Identifier,
    pub(crate) dimension_type: Identifier,
    pub(crate) colors: Vec<u8>,
}

impl NewMapData {
    /// Creates blank map data matching Vanilla `MapItem.create`.
    pub(crate) fn blank(
        world: &World,
        origin: BlockPos,
        scale: i8,
        tracking_position: bool,
        unlimited_tracking: bool,
    ) -> Self {
        Self {
            origin,
            scale,
            tracking_position,
            unlimited_tracking,
            world: world.key.clone(),
            dimension_type: world.dimension_type.key.clone(),
            colors: vec![0; MAP_COLOR_COUNT],
        }
    }
}

#[derive(SchemaWrite, SchemaRead)]
struct PersistentMapData {
    last_map_id: i32,
    maps: Vec<PersistentMapItemSavedData>,
}

#[derive(SchemaWrite, SchemaRead)]
struct PersistentMapItemSavedData {
    id: i32,
    center_x: i32,
    center_z: i32,
    world: Identifier,
    dimension_type: Identifier,
    tracking_position: bool,
    unlimited_tracking: bool,
    scale: i8,
    colors: Vec<u8>,
    locked: bool,
}

impl DomainMapData {
    /// Loads one map store through each domain's default-world persistence boundary.
    pub(crate) async fn load(
        domains: &[ResolvedDomainConfig],
        worlds: &WorldMap,
    ) -> io::Result<Self> {
        let mut map_data = BTreeMap::new();
        for domain in domains {
            let world = domain_default_world(worlds, &domain.name)?;
            let store = MapDataStore::load(world.saved_data.clone())
                .await
                .map_err(|error| map_data_io_error(&domain.name, error))?;
            map_data.insert(domain.name.clone(), Arc::new(store));
        }
        Ok(Self { domains: map_data })
    }

    /// Creates unpersisted map stores for tests and independently constructed servers.
    #[cfg(test)]
    pub(crate) fn ephemeral(domains: &[ResolvedDomainConfig]) -> Self {
        let domains = domains
            .iter()
            .map(|domain| (domain.name.clone(), Arc::new(MapDataStore::ephemeral())))
            .collect();
        Self { domains }
    }

    /// Returns the map store owned by `domain`.
    pub(crate) fn get(&self, domain: &str) -> Option<&Arc<MapDataStore>> {
        self.domains.get(domain)
    }

    /// Persists one domain's map data when it changed.
    pub(crate) async fn save(&self, domain: &str) -> io::Result<bool> {
        let Some(map_data) = self.domains.get(domain) else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("domain '{domain}' has no map-data store"),
            ));
        };
        map_data
            .save()
            .await
            .map_err(|error| map_data_io_error(domain, error))
    }
}

impl MapDataStore {
    async fn load(saved_data: SavedDataManager) -> io::Result<Self> {
        let loader = saved_data.clone();
        let persistent = spawn_blocking(move || {
            loader.sync_load_wincode::<PersistentMapData>(saved_data_names::MAP_DATA)
        })
        .await
        .map_err(|error| io::Error::other(format!("map-data load task failed: {error}")))??;

        let state = persistent.map_or_else(
            || {
                Ok(MapDataState {
                    last_map_id: -1,
                    maps: BTreeMap::new(),
                    revision: 0,
                    saved_revision: 0,
                })
            },
            MapDataState::from_persistent,
        )?;
        Ok(Self {
            saved_data,
            state: SyncRwLock::new(state),
            tracking: SyncMutex::new(MapTrackingState::default()),
        })
    }

    pub(crate) fn ephemeral() -> Self {
        Self {
            saved_data: SavedDataManager::new(None),
            state: SyncRwLock::new(MapDataState {
                last_map_id: -1,
                maps: BTreeMap::new(),
                revision: 0,
                saved_revision: 0,
            }),
            tracking: SyncMutex::new(MapTrackingState::default()),
        }
    }

    /// Allocates and stores a fresh map centered with Vanilla's grid formula.
    pub(crate) fn create_map(&self, map: NewMapData) -> io::Result<MapId> {
        let mut ids = self.create_maps(vec![map])?;
        ids.pop()
            .ok_or_else(|| io::Error::other("single map allocation returned no ID"))
    }

    /// Allocates a batch without publishing a partial prefix on failure.
    pub(crate) fn create_maps(&self, maps: Vec<NewMapData>) -> io::Result<Vec<MapId>> {
        if maps.is_empty() {
            return Ok(Vec::new());
        }
        for map in &maps {
            if map.colors.len() != MAP_COLOR_COUNT {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "map color buffer has length {}, expected {MAP_COLOR_COUNT}",
                        map.colors.len()
                    ),
                ));
            }
        }
        let mut state = self.state.write();
        let Ok(map_count) = i32::try_from(maps.len()) else {
            return Err(io::Error::other("map allocation batch is too large"));
        };
        let Some(last_new_id) = state.last_map_id.checked_add(map_count) else {
            return Err(io::Error::other("map ID space is exhausted"));
        };
        for id in (state.last_map_id + 1)..=last_new_id {
            if state.maps.contains_key(&id) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("map ID {id} is already allocated"),
                ));
            }
        }
        let first_id = state.last_map_id + 1;
        let mut ids = Vec::with_capacity(maps.len());
        for (offset, map) in maps.into_iter().enumerate() {
            let id = first_id + offset as i32;
            let (center_x, center_z) = MapItemSavedData::fresh_center(map.origin, map.scale);
            state.maps.insert(
                id,
                MapItemSavedData {
                    center_x,
                    center_z,
                    world: map.world,
                    dimension_type: map.dimension_type,
                    tracking_position: map.tracking_position,
                    unlimited_tracking: map.unlimited_tracking,
                    scale: map.scale,
                    colors: map.colors,
                    locked: false,
                },
            );
            ids.push(MapId::new(id));
        }
        if !ids.is_empty() {
            state.last_map_id = last_new_id;
            state.revision = state.revision.wrapping_add(1);
        }
        Ok(ids)
    }

    async fn save(&self) -> io::Result<bool> {
        let Some((revision, persistent)) = self.persistent_snapshot() else {
            return Ok(false);
        };
        let saver = self.saved_data.clone();
        spawn_blocking(move || saver.sync_save_wincode(saved_data_names::MAP_DATA, &persistent))
            .await
            .map_err(|error| io::Error::other(format!("map-data save task failed: {error}")))??;

        let mut state = self.state.write();
        if state.revision == revision {
            state.saved_revision = revision;
        }
        Ok(true)
    }

    fn persistent_snapshot(&self) -> Option<(u64, PersistentMapData)> {
        let state = self.state.read();
        if state.revision == state.saved_revision {
            return None;
        }
        let maps = state
            .maps
            .iter()
            .map(|(&id, map)| PersistentMapItemSavedData {
                id,
                center_x: map.center_x,
                center_z: map.center_z,
                world: map.world.clone(),
                dimension_type: map.dimension_type.clone(),
                tracking_position: map.tracking_position,
                unlimited_tracking: map.unlimited_tracking,
                scale: map.scale,
                colors: map.colors.clone(),
                locked: map.locked,
            })
            .collect();
        Some((
            state.revision,
            PersistentMapData {
                last_map_id: state.last_map_id,
                maps,
            },
        ))
    }
}

impl MapDataState {
    fn from_persistent(persistent: PersistentMapData) -> io::Result<Self> {
        if persistent.last_map_id < -1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid last map ID {}", persistent.last_map_id),
            ));
        }
        let mut maps = BTreeMap::new();
        for map in persistent.maps {
            if map.id < 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid negative map ID {}", map.id),
                ));
            }
            if map.colors.len() != MAP_COLOR_COUNT {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "map {} color buffer has length {}, expected {MAP_COLOR_COUNT}",
                        map.id,
                        map.colors.len()
                    ),
                ));
            }
            let id = map.id;
            let previous = maps.insert(
                id,
                MapItemSavedData {
                    center_x: map.center_x,
                    center_z: map.center_z,
                    world: map.world,
                    dimension_type: map.dimension_type,
                    tracking_position: map.tracking_position,
                    unlimited_tracking: map.unlimited_tracking,
                    scale: map.scale.clamp(0, 4),
                    colors: map.colors,
                    locked: map.locked,
                },
            );
            if previous.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("duplicate map ID {id}"),
                ));
            }
        }
        if maps
            .last_key_value()
            .is_some_and(|(&id, _)| id > persistent.last_map_id)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "last map ID precedes an allocated map",
            ));
        }
        Ok(Self {
            last_map_id: persistent.last_map_id,
            maps,
            revision: 0,
            saved_revision: 0,
        })
    }
}

impl MapItemSavedData {
    pub(crate) fn fresh_center(origin: BlockPos, scale: i8) -> (i32, i32) {
        let scaling = 1_i32.wrapping_shl(u32::from(scale as u8) & 31);
        let size = 128_i32.wrapping_mul(scaling);
        let area_x = ((f64::from(origin.x()) + 64.0) / f64::from(size)).floor() as i32;
        let area_z = ((f64::from(origin.z()) + 64.0) / f64::from(size)).floor() as i32;
        (
            area_x.wrapping_mul(size).wrapping_add(size / 2 - 64),
            area_z.wrapping_mul(size).wrapping_add(size / 2 - 64),
        )
    }
}

fn domain_default_world<'a>(worlds: &'a WorldMap, domain: &str) -> io::Result<&'a World> {
    worlds
        .default_world(domain)
        .map(AsRef::as_ref)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("domain '{domain}' has no loaded default world"),
            )
        })
}

fn map_data_io_error(domain: &str, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("map-data I/O failed for domain '{domain}': {error}"),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        env::temp_dir,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[test]
    fn fresh_map_centers_match_vanilla_grid() {
        assert_eq!(
            MapItemSavedData::fresh_center(BlockPos::new(0, 64, 0), 0),
            (0, 0)
        );
        assert_eq!(
            MapItemSavedData::fresh_center(BlockPos::new(500, 64, -500), 2),
            (704, -320)
        );
    }

    #[test]
    fn map_ids_are_domain_store_local_and_start_at_zero() {
        let first = MapDataStore::ephemeral();
        let second = MapDataStore::ephemeral();
        let colors = vec![0; MAP_COLOR_COUNT];

        let first_id = first
            .create_map(NewMapData {
                origin: BlockPos::new(0, 64, 0),
                scale: 1,
                tracking_position: true,
                unlimited_tracking: true,
                world: Identifier::vanilla_static("overworld"),
                dimension_type: Identifier::vanilla_static("overworld"),
                colors: colors.clone(),
            })
            .expect("first map allocation should succeed");
        let second_id = second
            .create_map(NewMapData {
                origin: BlockPos::new(10, 64, 10),
                scale: 1,
                tracking_position: true,
                unlimited_tracking: true,
                world: Identifier::vanilla_static("overworld"),
                dimension_type: Identifier::vanilla_static("overworld"),
                colors,
            })
            .expect("second domain map allocation should succeed");

        assert_eq!(first_id.id(), 0);
        assert_eq!(second_id.id(), 0);
    }

    #[test]
    fn persisted_map_scales_are_clamped_to_vanilla_bounds() {
        let maps = [-8, 9]
            .into_iter()
            .enumerate()
            .map(|(id, scale)| PersistentMapItemSavedData {
                id: id as i32,
                center_x: 0,
                center_z: 0,
                world: Identifier::vanilla_static("overworld"),
                dimension_type: Identifier::vanilla_static("overworld"),
                tracking_position: true,
                unlimited_tracking: false,
                scale,
                colors: vec![0; MAP_COLOR_COUNT],
                locked: false,
            })
            .collect();
        let state = MapDataState::from_persistent(PersistentMapData {
            last_map_id: 1,
            maps,
        })
        .expect("otherwise valid persisted maps should load");

        assert_eq!(state.maps[&0].scale, 0);
        assert_eq!(state.maps[&1].scale, 4);
    }

    #[tokio::test]
    async fn map_data_round_trips_and_continues_the_persistent_id_sequence() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after Unix epoch")
            .as_nanos();
        let path = temp_dir().join(format!("steel-map-data-round-trip-{unique}"));
        let saved_data = SavedDataManager::new(Some(path.as_path()));
        let store = MapDataStore::load(saved_data.clone())
            .await
            .expect("empty map data should load");
        let mut colors = vec![0; MAP_COLOR_COUNT];
        colors[321] = 47;
        let world = Identifier::vanilla_static("overworld");
        let dimension_type = Identifier::vanilla_static("overworld");

        let first_id = store
            .create_map(NewMapData {
                origin: BlockPos::new(500, 64, -500),
                scale: 2,
                tracking_position: true,
                unlimited_tracking: false,
                world: world.clone(),
                dimension_type: dimension_type.clone(),
                colors: colors.clone(),
            })
            .expect("map allocation should succeed");
        assert!(store.save().await.expect("changed map data should save"));

        let reloaded = MapDataStore::load(saved_data)
            .await
            .expect("saved map data should reload");
        {
            let state = reloaded.state.read();
            let map = state
                .maps
                .get(&first_id.id())
                .expect("saved map should be present after reload");
            assert_eq!((map.center_x, map.center_z), (704, -320));
            assert_eq!(map.world, world);
            assert_eq!(map.dimension_type, dimension_type);
            assert!(map.tracking_position);
            assert!(!map.unlimited_tracking);
            assert_eq!(map.scale, 2);
            assert_eq!(map.colors, colors);
            assert!(!map.locked);
        }

        let next_id = reloaded
            .create_map(NewMapData {
                origin: BlockPos::new(0, 64, 0),
                scale: 0,
                tracking_position: true,
                unlimited_tracking: true,
                world: Identifier::vanilla_static("overworld"),
                dimension_type: Identifier::vanilla_static("overworld"),
                colors: vec![0; MAP_COLOR_COUNT],
            })
            .expect("map allocation after reload should succeed");
        assert_eq!(next_id.id(), first_id.id() + 1);
    }
}
