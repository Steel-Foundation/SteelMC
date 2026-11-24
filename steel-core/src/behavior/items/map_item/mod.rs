use std::sync::Arc;

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::{
    data_components::vanilla_components::MAP_ID, item_stack::ItemStack, sound_events,
    stat::vanilla_stat_types, vanilla_items,
};
use steel_utils::{BlockPos, locks::SyncMutex};

use crate::behavior::UseOnContext;
use crate::inventory::container::Container;
use crate::{
    behavior::{InteractionResult, InventoryTickContext, ItemBehavior, UseItemContext},
    entity::Entity,
    maps::MapItemSavedData,
    player::player_inventory::Prediction,
    world::World,
};
use steel_registry::equipment::EquipmentSlotType;
use steel_registry::{blocks::block_state_ext::BlockStateExt, vanilla_block_tags::BlockTag};

/// Creates a new filled map when an empty map is used.
#[item_behavior]
pub struct EmptyMapItem;

impl ItemBehavior for EmptyMapItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let player = context.player;
        let world = context.world;
        context
            .inv
            .with_item(|stack| stack.consume_one(player.has_infinite_materials()));
        player.award_stat(&vanilla_stat_types::ITEM_USED, &vanilla_items::MAP);
        world.play_sound_at(
            &sound_events::UI_CARTOGRAPHY_TABLE_TAKE_RESULT,
            SoundSource::Players,
            player.position(),
            1.0,
            1.0,
            None,
        );
        let position = BlockPos::containing(
            player.position().x,
            player.position().y,
            player.position().z,
        );
        let mut map = MapItem::create(world, position.0.x, position.0.z, 0, true, false);
        let leftover = context.inv.with_inventory(|inventory| {
            if inventory.get_item_in_hand(context.hand).is_empty() {
                inventory.set_item_in_hand(context.hand, map);
                return None;
            }
            if inventory.add(&mut map) {
                None
            } else {
                Some(map)
            }
        });
        if let Some(map) = leftover {
            let _ = player.drop_item(map, false, Prediction::ServerOnly);
        }
        InteractionResult::Success
    }
}

/// Filled-map item behavior and saved-data creation.
#[item_behavior]
pub struct MapItem;

impl MapItem {
    /// Creates a filled map centered on the requested scale grid.
    pub fn create(
        world: &World,
        origin_x: i32,
        origin_z: i32,
        scale: i8,
        tracking_position: bool,
        unlimited_tracking: bool,
    ) -> ItemStack {
        let mut stack = ItemStack::new(&vanilla_items::FILLED_MAP);
        Self::apply_new_saved_data(
            world,
            &mut stack,
            origin_x,
            origin_z,
            scale,
            tracking_position,
            unlimited_tracking,
        );
        stack
    }

    /// Adds saved map data to the supplied stack without changing its item type.
    pub fn apply_new_saved_data(
        world: &World,
        stack: &mut ItemStack,
        origin_x: i32,
        origin_z: i32,
        scale: i8,
        tracking_position: bool,
        unlimited_tracking: bool,
    ) {
        let data = MapItemSavedData::create_fresh(
            f64::from(origin_x),
            f64::from(origin_z),
            scale,
            tracking_position,
            unlimited_tracking,
            world.key.clone(),
        );
        stack.set(MAP_ID, world.add_map_data(data));
    }

    /// Resolves this stack's map data in the world's domain.
    pub fn get_saved_data(
        world: &World,
        stack: &ItemStack,
    ) -> Option<Arc<SyncMutex<MapItemSavedData>>> {
        world.get_map_data(*stack.get(MAP_ID)?)
    }
}

impl ItemBehavior for MapItem {
    // TODO: Add map crafting and cartography-table transformations in a follow-up PR.
    fn is_map_item(&self) -> bool {
        true
    }
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let pos = context.hit_result.block_pos;
        if !context
            .world
            .get_block_state(pos)
            .get_block()
            .has_tag(&BlockTag::BANNERS)
        {
            return super::block_transformer::use_on(context);
        }
        let data = context
            .inv
            .with_item(|stack| Self::get_saved_data(context.world, stack));
        if data.is_some_and(|data| !data.lock().toggle_banner(context.world, pos)) {
            return InteractionResult::Fail;
        }
        InteractionResult::Success
    }

    fn inventory_tick(&self, context: &mut InventoryTickContext<'_>) {
        let Some(stack) = context.with_item(|stack| stack.copy_with_count(stack.count())) else {
            return;
        };
        let Some(data) = Self::get_saved_data(context.world, &stack) else {
            return;
        };
        let Some(player) = context.owner.as_player() else {
            return;
        };
        let Some(player) = context.world.players.get_by_uuid(&player.gameprofile.id()) else {
            return;
        };
        MapItemSavedData::tick_carried_by(&data, Arc::clone(&player), &stack, None);
        if context
            .slot
            .is_some_and(|slot| slot.slot_type() == EquipmentSlotType::Hand)
            && let Err(error) = MapItemSavedData::update(&data, context.world, &player)
        {
            log::error!("Could not retain map terrain update: {error}");
        }
    }
}
