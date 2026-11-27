//! Ink sac and glow ink sac sign applicators.

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_events::{ITEM_GLOW_INK_SAC_USE, ITEM_INK_SAC_USE};
use steel_utils::types::SignTextSlot;

use crate::behavior::{ItemBehavior, SignApplicator};
use crate::block_entity::{BlockEntity, entities::SignBlockEntity};
use crate::player::Player;
use crate::world::World;

fn apply_glow(world: &World, sign: &SignBlockEntity, slot: SignTextSlot, glowing: bool) -> bool {
    if !sign.set_glowing(slot, glowing) {
        return false;
    }

    let sound = if glowing {
        &ITEM_GLOW_INK_SAC_USE
    } else {
        &ITEM_INK_SAC_USE
    };
    world.play_sound(
        sound,
        SoundSource::Blocks,
        sign.get_block_pos(),
        1.0,
        1.0,
        None,
    );
    true
}

/// Removes glow from sign text.
#[item_behavior]
pub struct InkSacItem;

impl ItemBehavior for InkSacItem {
    fn as_sign_applicator(&self) -> Option<&dyn SignApplicator> {
        Some(self)
    }
}

impl SignApplicator for InkSacItem {
    fn try_apply_to_sign(
        &self,
        world: &World,
        sign: &SignBlockEntity,
        slot: SignTextSlot,
        _item: &ItemStack,
        _player: &Player,
    ) -> bool {
        apply_glow(world, sign, slot, false)
    }
}

/// Makes sign text glow.
#[item_behavior]
pub struct GlowInkSacItem;

impl ItemBehavior for GlowInkSacItem {
    fn as_sign_applicator(&self) -> Option<&dyn SignApplicator> {
        Some(self)
    }
}

impl SignApplicator for GlowInkSacItem {
    fn try_apply_to_sign(
        &self,
        world: &World,
        sign: &SignBlockEntity,
        slot: SignTextSlot,
        _item: &ItemStack,
        _player: &Player,
    ) -> bool {
        apply_glow(world, sign, slot, true)
    }
}
