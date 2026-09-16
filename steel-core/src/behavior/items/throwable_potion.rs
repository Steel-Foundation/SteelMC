//! Splash/lingering potion item behaviors (`SplashPotionItem`, `LingeringPotionItem`).

use std::borrow::Cow;
use std::sync::Arc;

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::item_stack::ItemStack;
use steel_registry::{sound_events, vanilla_entities};
use text_components::TextComponent;

use super::dynamic_name::potion_name;
use super::potion::potion_use_on;
use super::throw_projectile::{ThrowParams, throw_item_projectile};
use crate::behavior::context::{InteractionResult, UseItemContext, UseOnContext};
use crate::behavior::item::ItemBehavior;
use crate::entity::entities::SplashPotionEntity;
use crate::entity::next_entity_id;

const THROW: ThrowParams = ThrowParams {
    sound: &sound_events::ENTITY_SPLASH_POTION_THROW,
    sound_source: SoundSource::Players,
    sound_volume: 0.5,
    pitch_offset: -20.0,
    power: 0.5,
    uncertainty: 1.0,
};

/// Splash-potion behavior providing Vanilla's potion-content-dependent name.
// TODO: Implement the `ProjectileItem` dispenser dispatch.
// TODO: Add the inherited water default instance when Steel has item-specific
// default-stack factories.
#[item_behavior]
pub struct SplashPotionItem;

impl ItemBehavior for SplashPotionItem {
    fn get_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        potion_name(stack)
    }

    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        potion_use_on(context)
    }

    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let world = context.world;
        throw_item_projectile(context, &THROW, |spawn_pos| {
            SplashPotionEntity::new(
                &vanilla_entities::SPLASH_POTION,
                next_entity_id(),
                spawn_pos,
                Arc::downgrade(world),
            )
        });
        InteractionResult::Success
    }
}

/// Lingering-potion behavior providing Vanilla's potion-content-dependent name.
// TODO: Implement `use` and the `ProjectileItem` dispenser dispatch. Blocked on
// `AreaEffectCloud` (not implemented) and the lingering potion entity.
// TODO: Add the inherited water default instance when Steel has item-specific
// default-stack factories.
#[item_behavior]
pub struct LingeringPotionItem;

impl ItemBehavior for LingeringPotionItem {
    fn get_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        potion_name(stack)
    }

    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        potion_use_on(context)
    }
}
