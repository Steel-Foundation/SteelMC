use crate::behavior::ItemBehavior;
use crate::world::World;
use std::sync::Arc;
use steel_macros::item_behavior;
use steel_registry::data_components::vanilla_components::MAP_ID;
use steel_registry::data_components::{DataComponentType, MapId, vanilla_components};
use steel_registry::dimension_type::DimensionTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_items;

/// The non-empty map item
#[item_behavior]
pub struct MapItem;

impl MapItem {
    pub(crate) fn create(
        level: &Arc<World>,
        origin_x: i32,
        origin_z: i32,
        scale: i8,
        track_position: bool,
        unlimited_tracking: bool,
    ) -> ItemStack {
        let mut map = ItemStack::new(&vanilla_items::FILLED_MAP);
        let new_id = MapItem::create_new_saved_data(
            level,
            origin_x,
            origin_z,
            scale,
            track_position,
            unlimited_tracking,
            level.dimension_type,
        );
        map.set(MAP_ID, new_id);
        map
    }

    fn create_new_saved_data(
        level: &Arc<World>,
        origin_x: i32,
        origin_z: i32,
        scale: i8,
        track_position: bool,
        unlimited_tracking: bool,
        dimension: DimensionTypeRef,
    ) -> MapId {
    }
}

impl ItemBehavior for MapItem {}
