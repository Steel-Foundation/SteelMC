use std::sync::Arc;

use steel_registry::data_components::vanilla_components::MapId;
use steel_utils::locks::SyncMutex;

use super::MapItemSavedData;
use crate::world::World;
use crate::{inventory::container::Container, player::Player};
use steel_registry::data_components::vanilla_components::MAP_ID;

impl World {
    /// Resolves a map through the shared map namespace of this world's domain.
    pub fn get_map_data(&self, id: MapId) -> Option<Arc<SyncMutex<MapItemSavedData>>> {
        self.maps.read().get(id)
    }

    /// Allocates a domain map ID and installs its initial saved data together.
    pub fn add_map_data(&self, data: MapItemSavedData) -> MapId {
        self.maps.read().insert(data)
    }
}

impl Player {
    pub(crate) fn synchronize_special_item_updates(&self) {
        let ids = self
            .inventory
            .lock()
            .items()
            .iter()
            .filter(|stack| !stack.is_empty())
            .filter_map(|stack| stack.get(MAP_ID).copied())
            .collect::<Vec<_>>();
        let world = self.get_world();
        for id in ids {
            let Some(data) = world.get_map_data(id) else {
                continue;
            };
            let packet = data.lock().update_packet(id, self);
            if let Some(packet) = packet {
                self.send_packet(packet);
            }
        }
    }
}
