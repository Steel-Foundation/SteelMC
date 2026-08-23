//! Empty map item (`EmptyMapItem`).

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::sound_events;

use crate::behavior::context::{InteractionResult, UseItemContext};
use crate::behavior::item::ItemBehavior;
use crate::behavior::item_utils::create_filled_result;
use crate::behavior::items::map_item::MapItem;
use crate::entity::Entity as _;

/// Vanilla `EmptyMapItem`.
#[item_behavior(class = "EmptyMapItem")]
pub struct EmptyMapItem;

impl ItemBehavior for EmptyMapItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let player = context.player;
        let world = context.world;
        world.play_sound_at(
            &sound_events::UI_CARTOGRAPHY_TABLE_TAKE_RESULT,
            SoundSource::Players,
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
        create_filled_result(context, map, false);
        InteractionResult::Success
    }
}
