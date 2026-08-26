use std::{
    ops::{Add, Mul},
    sync::Arc,
};

use steel_macros::item_behavior;
use steel_registry::{blocks::BlockRef, items::item::BlockHitResult};

use crate::{
    behavior::{BlockItem, InteractionResult, ItemBehavior, UseItemContext, UseOnContext},
    entity::Entity,
    player::Player,
    world::{ClipBlockShape, ClipFluid, World},
};
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

fn get_player_pov_hit_result(
    world: &Arc<World>,
    player: &Player,
    fluid: ClipFluid,
) -> BlockHitResult {
    let from = player.position().with_y(player.get_eye_y());
    let to = from.add(
        player
            .calculate_view_vector(player.rotation().1, player.rotation().0)
            .mul(player.block_interaction_range()),
    );
    let c_r = world.clip(from, to, ClipBlockShape::Outline, fluid);
    BlockHitResult {
        location: c_r.location,
        direction: c_r.direction,
        block_pos: c_r.block_pos,
        miss: c_r.miss,
        inside: c_r.inside,
        world_border_hit: c_r.world_border_hit,
    }
}
