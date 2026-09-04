//! Solid bucket item behavior implementation.

use steel_macros::item_behavior;
use steel_registry::{
    blocks::BlockRef, item_stack::ItemStack, sound_event::SoundEventRef, vanilla_items,
};

use crate::behavior::items::BlockItem;
use crate::behavior::{InteractionResult, ItemBehavior, UseOnContext};

/// Behavior for buckets that place a solid block, such as powder snow.
#[item_behavior]
pub struct SolidBucketItem {
    #[json_arg(vanilla_blocks, json = "block")]
    _block: BlockRef,
    #[json_arg(sound_events, json = "place_sound")]
    place_sound: SoundEventRef,
    base: BlockItem,
}

impl SolidBucketItem {
    /// Creates a solid bucket item behavior.
    #[must_use]
    pub const fn new(block: BlockRef, place_sound: SoundEventRef) -> Self {
        Self {
            _block: block,
            place_sound,
            base: BlockItem::new(block),
        }
    }
}

impl ItemBehavior for SolidBucketItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let result = self.base.place_with_sound_and_block(
            context.build_place_context(),
            BlockItem::place_block,
            self.place_sound,
        );

        if matches!(result, InteractionResult::Success) && !context.player.has_infinite_materials()
        {
            context
                .inv
                .with_item(|item| *item = ItemStack::new(&vanilla_items::BUCKET));
        }

        result
    }
}
