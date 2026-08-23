//! Filled map item (`MapItem`).

use std::sync::Arc;

use steel_macros::item_behavior;
use steel_registry::data_components::components::MapPostProcessing;
use steel_registry::data_components::vanilla_components::{MAP_ID, MAP_POST_PROCESSING};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_items;

use crate::behavior::item::ItemBehavior;
use crate::player::Player;
use crate::world::World;
use crate::world::map_data::{MapDataStore, MapItemSavedData};

/// Vanilla `MapItem`.
#[item_behavior(class = "MapItem")]
pub struct MapItem;

impl MapItem {
    /// Vanilla `MapItem.create`.
    #[must_use]
    pub fn create(
        world: &Arc<World>,
        origin_x: i32,
        origin_z: i32,
        scale: u8,
        tracking_position: bool,
        unlimited_tracking: bool,
    ) -> ItemStack {
        let data = MapItemSavedData::create_fresh(
            origin_x,
            origin_z,
            scale,
            tracking_position,
            unlimited_tracking,
            world.key.clone(),
        );
        let id = world.map_data.lock().insert_new(data);
        let mut stack = ItemStack::new(&vanilla_items::FILLED_MAP);
        stack.set(MAP_ID, id);
        stack
    }

    /// Vanilla `MapItem.getSavedData`.
    #[must_use]
    pub fn saved_data<'a>(
        stack: &ItemStack,
        store: &'a MapDataStore,
    ) -> Option<&'a MapItemSavedData> {
        stack.get(MAP_ID).and_then(|id| store.get(*id))
    }

    /// Vanilla `MapItem.onCraftedPostProcess`.
    pub fn apply_post_processing(stack: &mut ItemStack, world: &World) {
        let Some(post_processing) = stack.get(MAP_POST_PROCESSING).copied() else {
            return;
        };
        stack.remove(MAP_POST_PROCESSING);
        let Some(id) = stack.get(MAP_ID).copied() else {
            return;
        };
        let mut store = world.map_data.lock();
        let Some(original) = store.get(id) else {
            return;
        };
        let data = match post_processing {
            MapPostProcessing::Lock => original.locked(),
            MapPostProcessing::Scale => original.scaled(),
        };
        let new_id = store.insert_new(data);
        drop(store);
        stack.set(MAP_ID, new_id);
    }
}

impl ItemBehavior for MapItem {
    fn on_crafted_by(&self, stack: &mut ItemStack, player: &Player) {
        Self::apply_post_processing(stack, &player.get_world());
    }
}
