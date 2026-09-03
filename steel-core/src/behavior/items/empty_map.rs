use crate::behavior::items::map::MapItem;
use crate::behavior::{InteractionResult, ItemBehavior, UseItemContext};
use crate::entity::Entity;
use crate::inventory::container::Container;
use crate::world::World;
use std::sync::Arc;
use steel_macros::item_behavior;
use steel_registry::sound_events;
use steel_registry::stat::vanilla_stat_types;

/// The empty map item
#[item_behavior]
pub struct EmptyMapItem;

impl ItemBehavior for EmptyMapItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let world = context.world;
        let player = context.player;

        let (item_ref, hand_empty) = context.inv.with_item(|item| {
            item.shrink(1);
            (item.item(), item.is_empty())
        });

        player.award_stat(&vanilla_stat_types::ITEM_USED, item_ref);

        world.play_sound_at(
            &sound_events::UI_CARTOGRAPHY_TABLE_TAKE_RESULT,
            player.sound_source(),
            player.position(),
            1.0,
            1.0,
            None,
        );

        let map = MapItem::create(
            world,
            player.block_position().x(),
            player.block_position().z(),
            0,
            true,
            false,
        );

        if hand_empty {
            // TODO
        }

        player.add_item_or_drop(map);

        InteractionResult::Success
    }
}
