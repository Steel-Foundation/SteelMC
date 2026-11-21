use std::{collections::BTreeMap, io, sync::Arc};

use serde::{Deserialize, Serialize};
use steel_registry::{
    DyeColor, data_components::vanilla_components::MapId, vanilla_map_decoration_types,
};
use steel_utils::{
    BlockPos, Identifier,
    locks::{AsyncMutex, SyncMutex},
    saved_data::{SavedDataManager, names},
};
use wincode::{SchemaRead, SchemaWrite};

use super::{MAP_SIZE, MapBanner, MapFrame, MapItemSavedData};
use crate::server::worlds::WorldMap;

#[derive(Deserialize, Serialize)]
struct PersistentMapIndex {
    last_id: i32,
    maps: Vec<i32>,
}

impl Default for PersistentMapIndex {
    fn default() -> Self {
        Self {
            last_id: -1,
            maps: Vec::new(),
        }
    }
}

#[derive(SchemaRead, SchemaWrite)]
struct PersistentMap {
    dimension: Identifier,
    center_x: i32,
    center_z: i32,
    scale: i8,
    tracking_position: bool,
    unlimited_tracking: bool,
    locked: bool,
    colors: Vec<u8>,
    banners: Vec<PersistentBanner>,
    frames: Vec<PersistentFrame>,
}

#[derive(SchemaRead, SchemaWrite)]
struct PersistentBanner {
    pos: [i32; 3],
    color: i32,
    name: Option<String>,
}

#[derive(SchemaRead, SchemaWrite)]
struct PersistentFrame {
    pos: [i32; 3],
    rotation: i32,
    entity_id: i32,
}

impl PersistentMap {
    fn capture(data: &MapItemSavedData) -> io::Result<Self> {
        let banners = data
            .banners
            .values()
            .map(|banner| {
                Ok(PersistentBanner {
                    pos: banner.pos.0.to_array(),
                    color: banner.color.id(),
                    name: banner
                        .name
                        .as_ref()
                        .map(serde_json::to_string)
                        .transpose()
                        .map_err(io::Error::other)?,
                })
            })
            .collect::<io::Result<_>>()?;
        let frames = data
            .frames
            .values()
            .map(|frame| PersistentFrame {
                pos: frame.pos.0.to_array(),
                rotation: frame.rotation,
                entity_id: frame.entity_id,
            })
            .collect();
        Ok(Self {
            dimension: data.dimension.clone(),
            center_x: data.center_x,
            center_z: data.center_z,
            scale: data.scale,
            tracking_position: data.tracking_position,
            unlimited_tracking: data.unlimited_tracking,
            locked: data.locked,
            colors: data.colors.to_vec(),
            banners,
            frames,
        })
    }

    fn restore(self) -> io::Result<MapItemSavedData> {
        let mut data = MapItemSavedData::create_fresh(
            0.0,
            0.0,
            self.scale.clamp(0, MapItemSavedData::MAX_SCALE),
            self.tracking_position,
            self.unlimited_tracking,
            self.dimension,
        );
        data.center_x = self.center_x;
        data.center_z = self.center_z;
        data.locked = self.locked;
        if self.colors.len() == MAP_SIZE * MAP_SIZE {
            data.colors.copy_from_slice(&self.colors);
        }
        for banner in self.banners {
            let banner = MapBanner {
                pos: BlockPos::new(banner.pos[0], banner.pos[1], banner.pos[2]),
                color: DyeColor::by_id(banner.color),
                name: banner
                    .name
                    .map(|name| serde_json::from_str(&name))
                    .transpose()
                    .map_err(io::Error::other)?,
            };
            data.add_decoration(
                banner.decoration_type(),
                banner.key(),
                f64::from(banner.pos.0.x),
                f64::from(banner.pos.0.z),
                180.0,
                None,
                banner.name.clone(),
            );
            data.banners.insert(banner.pos.0.to_array(), banner);
        }
        for frame in self.frames {
            let frame = MapFrame {
                pos: BlockPos::new(frame.pos[0], frame.pos[1], frame.pos[2]),
                rotation: frame.rotation,
                entity_id: frame.entity_id,
            };
            data.add_decoration(
                &vanilla_map_decoration_types::FRAME,
                format!("frame-{}", frame.entity_id),
                f64::from(frame.pos.0.x),
                f64::from(frame.pos.0.z),
                f64::from(frame.rotation),
                None,
                None,
            );
            data.frames.insert(frame.pos.0.to_array(), frame);
        }
        data.revision = 0;
        data.saved_revision = 0;
        Ok(data)
    }
}

struct MapState {
    last_id: i32,
    maps: BTreeMap<i32, Arc<SyncMutex<MapItemSavedData>>>,
    revision: u64,
    saved_revision: u64,
}

/// In-memory map access for ticks; the domain service performs all disk I/O.
pub(crate) struct MapStorage {
    state: SyncMutex<MapState>,
    save_lock: AsyncMutex<()>,
}

impl Default for MapStorage {
    fn default() -> Self {
        Self {
            state: SyncMutex::new(MapState {
                last_id: -1,
                maps: BTreeMap::new(),
                revision: 0,
                saved_revision: 0,
            }),
            save_lock: AsyncMutex::new(()),
        }
    }
}

impl MapStorage {
    async fn load(manager: &SavedDataManager) -> io::Result<Self> {
        let index: PersistentMapIndex = manager.load_or_default(names::MAP_INDEX).await?;
        let mut maps = BTreeMap::new();
        for id in index.maps {
            let Some(saved) = manager
                .load_wincode::<PersistentMap>(names::MAP_DATA.with_index(id))
                .await?
            else {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("map index references missing map {id}"),
                ));
            };
            if maps
                .insert(id, Arc::new(SyncMutex::new(saved.restore()?)))
                .is_some()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("duplicate map ID {id}"),
                ));
            }
        }
        Ok(Self {
            state: SyncMutex::new(MapState {
                last_id: index.last_id,
                maps,
                revision: 0,
                saved_revision: 0,
            }),
            save_lock: AsyncMutex::new(()),
        })
    }
    async fn save(&self, manager: &SavedDataManager) -> io::Result<usize> {
        let _save = self.save_lock.lock().await;
        let mut saved_count = 0;
        let (revision, saved_revision, index, maps) = {
            let state = self.state.lock();
            (
                state.revision,
                state.saved_revision,
                PersistentMapIndex {
                    last_id: state.last_id,
                    maps: state.maps.keys().copied().collect(),
                },
                state
                    .maps
                    .iter()
                    .map(|(&id, map)| (id, Arc::clone(map)))
                    .collect::<Vec<_>>(),
            )
        };
        for (id, map) in maps {
            let snapshot = {
                let map = map.lock();
                if map.revision == map.saved_revision {
                    None
                } else {
                    Some((map.revision, PersistentMap::capture(&map)?))
                }
            };
            let Some((revision, snapshot)) = snapshot else {
                continue;
            };
            manager
                .save_wincode(names::MAP_DATA.with_index(id), &snapshot)
                .await?;
            map.lock().saved_revision = revision;
            saved_count += 1;
        }
        if revision != saved_revision {
            // Every referenced pixel file exists before the index is published.
            manager.save(names::MAP_INDEX, &index).await?;
            self.state.lock().saved_revision = revision;
        }
        Ok(saved_count)
    }

    pub(crate) fn get(&self, id: MapId) -> Option<Arc<SyncMutex<MapItemSavedData>>> {
        self.state.lock().maps.get(&id.id()).map(Arc::clone)
    }

    pub(crate) fn insert(&self, map: MapItemSavedData) -> MapId {
        let mut state = self.state.lock();
        state.last_id = state.last_id.wrapping_add(1);
        let id = state.last_id;
        state.maps.insert(id, Arc::new(SyncMutex::new(map)));
        state.revision = state.revision.wrapping_add(1);
        MapId::new(id)
    }
}

/// One map namespace per domain, shared by all of its dimensions.
pub(crate) struct DomainMaps {
    domains: BTreeMap<String, Arc<MapStorage>>,
}

impl DomainMaps {
    pub(crate) async fn load(worlds: &WorldMap) -> io::Result<Self> {
        let mut domains = BTreeMap::new();
        for domain in worlds.domain_names() {
            let world = worlds
                .default_world(domain)
                .ok_or_else(|| io::Error::other(format!("domain {domain} has no default world")))?;
            // Preloading keeps inventory ticks free of filesystem work.
            let storage = Arc::new(MapStorage::load(&world.saved_data).await?);
            for world in worlds.values().filter(|world| world.domain() == domain) {
                *world.maps.write() = Arc::clone(&storage);
            }
            domains.insert(domain.to_owned(), storage);
        }
        Ok(Self { domains })
    }

    pub(crate) async fn save(&self, worlds: &WorldMap) -> io::Result<usize> {
        let mut saved_count = 0;
        for (domain, storage) in &self.domains {
            let world = worlds
                .default_world(domain)
                .ok_or_else(|| io::Error::other(format!("domain {domain} has no default world")))?;
            let lifecycle = Arc::clone(&world.loot_lifecycle.read());
            let _barrier = lifecycle.save_barrier().await;
            saved_count += storage.save(&world.saved_data).await?;
        }
        Ok(saved_count)
    }
}

#[cfg(test)]
mod tests {
    use std::{env::temp_dir, fs, io::ErrorKind};
    use tokio::runtime::Builder;

    use steel_registry::init_vanilla_registry;
    use steel_utils::saved_data::SavedDataManager;
    use text_components::TextComponent;

    use super::*;

    #[test]
    fn save_reload_retains_pixels_markers_and_id_allocation_and_only_writes_dirty_maps() {
        init_vanilla_registry();
        let root = temp_dir().join(format!("steel-map-store-{}", uuid::Uuid::new_v4()));
        let manager = SavedDataManager::new(Some(&root));
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let storage = MapStorage::load(&manager).await.expect("empty map store");
            let dimension = Identifier::new_static("survival", "the_nether");
            let mut data =
                MapItemSavedData::create_fresh(-193.0, 241.0, 2, true, true, dimension.clone());
            data.set_color(2, 3, 45);
            data.set_color(127, 127, 201);
            let banner = MapBanner {
                pos: BlockPos::new(-190, 64, 245),
                color: DyeColor::Blue,
                name: Some(TextComponent::plain("Named banner")),
            };
            data.banners.insert(banner.pos.0.to_array(), banner);
            let frame = MapFrame {
                pos: BlockPos::new(-191, 66, 246),
                rotation: 270,
                entity_id: 42,
            };
            data.frames.insert(frame.pos.0.to_array(), frame);
            let id = storage.insert(data);
            assert_eq!(storage.save(&manager).await.expect("first save"), 1);
            assert_eq!(storage.save(&manager).await.expect("unchanged save"), 0);
            drop(storage);

            let storage = MapStorage::load(&manager).await.expect("reload maps");
            let map = storage.get(id).expect("saved map");
            {
                let data = map.lock();
                assert_eq!(data.dimension, dimension);
                assert_eq!(data.scale, 2);
                assert_eq!(data.colors()[3 * 128 + 2], 45);
                assert_eq!(data.colors()[16_383], 201);
                assert_eq!(
                    data.banners.values().next().expect("banner").color,
                    DyeColor::Blue
                );
                assert!(data.banners.values().next().expect("banner").name.is_some());
                assert_eq!(data.frames.values().next().expect("frame").entity_id, 42);
                assert_eq!(data.decorations.len(), 2);
            }
            assert_eq!(storage.save(&manager).await.expect("clean reload"), 0);
            map.lock().set_color(2, 3, 46);
            assert_eq!(storage.save(&manager).await.expect("changed map"), 1);
            let next = storage.insert(MapItemSavedData::create_fresh(
                0.0, 0.0, 0, true, false, dimension,
            ));
            assert_eq!(next.id(), id.id() + 1);
            assert_eq!(storage.save(&manager).await.expect("new allocation"), 1);
            let reloaded = MapStorage::load(&manager).await.expect("second reload");
            assert_eq!(
                reloaded.get(id).expect("updated map").lock().colors()[3 * 128 + 2],
                46
            );
            assert!(reloaded.get(next).is_some());
        });
        fs::remove_dir_all(root).expect("remove fixture files");
    }

    #[test]
    fn an_index_never_silently_recreates_missing_map_pixels() {
        let root = temp_dir().join(format!("steel-missing-map-{}", uuid::Uuid::new_v4()));
        let manager = SavedDataManager::new(Some(&root));
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            manager
                .save(
                    names::MAP_INDEX,
                    &PersistentMapIndex {
                        last_id: 3,
                        maps: vec![3],
                    },
                )
                .await
                .expect("index");
            let Err(error) = MapStorage::load(&manager).await else {
                panic!("missing map data should fail")
            };
            assert_eq!(error.kind(), ErrorKind::NotFound);
        });
        fs::remove_dir_all(root).expect("remove fixture files");
    }
}

#[cfg(test)]
#[path = "generation_tests.rs"]
mod generation_tests;
