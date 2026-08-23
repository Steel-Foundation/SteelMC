//! Ink sac and glow ink sac item behaviors.
//!
//! Ports vanilla `InkSacItem` and `GlowInkSacItem` (`SignApplicator`), along with
//! the guards `SignBlock.useItemOn` applies before delegating to them.
//!
//! Vanilla dispatches these from the block side via the `SignApplicator` interface;
//! Steel implements them as item behaviors to match the existing `HoneycombItem`
//! sign-waxing path.

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::{
    sound_event::SoundEventRef,
    sound_events::{ITEM_GLOW_INK_SAC_USE, ITEM_INK_SAC_USE},
    vanilla_game_events,
};
use steel_utils::Downcast as _;

use crate::{
    behavior::{InteractionResult, ItemBehavior, UseOnContext, blocks::is_facing_front_text},
    block_entity::{BlockEntity, entities::SignBlockEntity},
    world::game_event::GameEventContext,
};

/// Shared applicator path for both ink sac variants.
fn apply_glow(context: &mut UseOnContext, glowing: bool) -> InteractionResult {
    let pos = context.hit_result.block_pos;

    let Some(block_entity) = context.world.get_block_entity(pos) else {
        return InteractionResult::Pass;
    };
    let Some(sign) = block_entity.downcast_ref::<SignBlockEntity>() else {
        return InteractionResult::Pass;
    };

    if !context.player.abilities.lock().may_build
        || sign.is_waxed()
        || sign.is_other_player_editing(context.player.gameprofile.id)
    {
        return InteractionResult::TryEmptyHandInteraction;
    }

    let state = context.world.get_block_state(pos);
    let is_front_text = is_facing_front_text(state, pos, context.player);

    if !sign.set_glowing(is_front_text, glowing) {
        return InteractionResult::TryEmptyHandInteraction;
    }

    sign.set_changed();
    if let Some(nbt) = sign.get_update_tag() {
        context
            .world
            .broadcast_block_entity_update(pos, sign.get_type(), nbt);
    }

    context.world.game_event(
        &vanilla_game_events::BLOCK_CHANGE,
        pos,
        &GameEventContext::new(Some(context.player), Some(state)),
    );

    context.inv.with_item(|item| item.shrink(1));

    context
        .world
        .play_sound(sound_for(glowing), SoundSource::Blocks, pos, 1.0, 1.0, None);

    InteractionResult::Success
}

const fn sound_for(glowing: bool) -> SoundEventRef {
    if glowing {
        &ITEM_GLOW_INK_SAC_USE
    } else {
        &ITEM_INK_SAC_USE
    }
}

/// Behavior for the ink sac item. Removes glow from sign text.
#[item_behavior]
pub struct InkSacItem;

impl ItemBehavior for InkSacItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        apply_glow(context, false)
    }
}

/// Behavior for the glow ink sac item. Makes sign text glow.
#[item_behavior]
pub struct GlowInkSacItem;

impl ItemBehavior for GlowInkSacItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        apply_glow(context, true)
    }
}
