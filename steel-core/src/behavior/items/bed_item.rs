use steel_macros::item_behavior;
use steel_registry::blocks::BlockRef;
use steel_utils::{BlockStateId, types::UpdateFlags};

use crate::behavior::ItemBehavior;
use crate::behavior::context::{BlockPlaceContext, InteractionResult, UseOnContext};
use crate::behavior::items::BlockItem;

/// Behavior for beds
#[item_behavior]
pub struct BedItem {
    #[json_arg(vanilla_blocks, json = "block")]
    _block: BlockRef,
    base: BlockItem,
}

impl BedItem {
    const PLACE_BLOCK_FLAGS: UpdateFlags = UpdateFlags::UPDATE_CLIENTS
        .union(UpdateFlags::UPDATE_IMMEDIATE)
        .union(UpdateFlags::UPDATE_KNOWN_SHAPE);

    /// New bed item behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            _block: block,
            base: BlockItem::new(block),
        }
    }

    fn place_block(context: &BlockPlaceContext<'_>, state: BlockStateId) -> bool {
        context
            .world
            .set_block(context.place_pos, state, Self::PLACE_BLOCK_FLAGS)
    }
}

impl ItemBehavior for BedItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        self.base.place_with(context, |place_context, state| {
            Self::place_block(place_context, state)
        })
    }
}
