use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::item_stack::ItemStack;
use steel_registry::{
    REGISTRY, blocks::block_state_ext::BlockStateExt, level_events,
    sound_events::ITEM_HONEYCOMB_WAX_ON, vanilla_game_events,
};
use steel_utils::types::{SignTextSlot, UpdateFlags};

use crate::{
    behavior::{
        InteractionResult, ItemBehavior, SignApplicator, UseOnContext,
        waxables::get_waxed_from_normal_variant,
    },
    block_entity::{
        BlockEntity,
        entities::{SignBlockEntity, SignText},
    },
    entity::Entity,
    player::Player,
    world::{World, game_event::GameEventContext},
};

use super::copper_chest_events::emit_connected_chest_block_change;

/// Behavior for the honeycomb item. Waxes copper blocks and signs.
#[item_behavior]
pub struct HoneycombItem;

impl ItemBehavior for HoneycombItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let pos = context.hit_result.block_pos;

        // Try block waxing first
        let old_block_state = context.world.get_block_state(pos);
        if let Some(waxed_block) = get_waxed_from_normal_variant(old_block_state.get_block()) {
            context.inv.with_item(ItemStack::shrink_one);
            // TODO: trigger CriteriaTriggers.ITEM_USED_ON_BLOCK advancement
            let waxed_state = REGISTRY
                .blocks
                .copy_matching_properties(old_block_state, waxed_block);
            context
                .world
                .set_block(pos, waxed_state, UpdateFlags::UPDATE_ALL);
            context.world.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                pos,
                &GameEventContext::new(Some(context.player.as_ref()), Some(waxed_state)),
            );
            context.world.level_event(
                level_events::PARTICLES_WAX_ON,
                pos,
                0,
                Some(context.player.id()),
            );
            emit_connected_chest_block_change(
                context.world,
                pos,
                old_block_state,
                context.player,
                Some(level_events::PARTICLES_WAX_ON),
            );
            return InteractionResult::Success;
        }

        InteractionResult::Pass
    }

    fn as_sign_applicator(&self) -> Option<&dyn SignApplicator> {
        Some(self)
    }
}

impl SignApplicator for HoneycombItem {
    fn can_apply_to_sign(&self, _text: &SignText, _item: &ItemStack, _player: &Player) -> bool {
        true
    }

    fn try_apply_to_sign(
        &self,
        world: &World,
        sign: &SignBlockEntity,
        _slot: SignTextSlot,
        _item: &ItemStack,
        _player: &Player,
    ) -> bool {
        if !sign.wax() {
            return false;
        }
        let pos = sign.get_block_pos();
        world.level_event(level_events::PARTICLES_WAX_ON, pos, 0, None);
        world.play_sound(
            &ITEM_HONEYCOMB_WAX_ON,
            SoundSource::Blocks,
            pos,
            1.0,
            1.0,
            None,
        );
        true
    }
}
