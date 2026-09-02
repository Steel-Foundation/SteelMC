use steel_macros::item_behavior;
use steel_registry::blocks::BlockRef;

use crate::behavior::item_utils::get_player_pov_hit_result;
use crate::behavior::{BlockItem, InteractionResult, ItemBehavior, UseItemContext, UseOnContext};
use crate::world::ClipFluid;

/// blockitem behavior for lily pad and frog spawn.
#[item_behavior]
pub struct PlaceOnWaterBlockItem {
    /// The block this item places.
    #[json_arg(vanilla_blocks, json = "block")]
    pub base: BlockItem,
}

impl PlaceOnWaterBlockItem {
    /// New block item behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            base: BlockItem::new(block),
        }
    }
}

impl ItemBehavior for PlaceOnWaterBlockItem {
    fn use_on(&self, _context: &mut UseOnContext) -> InteractionResult {
        InteractionResult::Pass
    }
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let mut hit_result =
            get_player_pov_hit_result(context.world, context.player, ClipFluid::SourceOnly);
        hit_result.block_pos = hit_result.block_pos.above();
        self.base.use_on(&mut UseOnContext {
            player: context.player,
            hand: context.hand,
            hit_result,
            world: context.world,
            inv: context.inv.clone(),
        })
    }
}
