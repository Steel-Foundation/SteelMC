//! Ender pearl item behavior (`EnderpearlItem`).
//!
//! Throwing an ender pearl spawns a [`EnderPearlEntity`] from the player's eye,
//! shot along their look direction, and consumes one pearl unless the player
//! has infinite materials. Mirrors vanilla `EnderpearlItem.use`.

use std::sync::Arc;

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::{sound_events, vanilla_entities};

use super::throw_projectile::{ThrowParams, throw_item_projectile};
use crate::behavior::context::{InteractionResult, UseItemContext};
use crate::behavior::item::ItemBehavior;
use crate::entity::entities::EnderPearlEntity;
use crate::entity::next_entity_id;

/// Vanilla `EnderpearlItem.use`'s sound, `PROJECTILE_SHOOT_POWER` and throw spread.
const THROW: ThrowParams = ThrowParams {
    sound: &sound_events::ENTITY_ENDER_PEARL_THROW,
    sound_source: SoundSource::Neutral,
    sound_volume: 0.5,
    y_offset: 0.0,
    power: 1.5,
    uncertainty: 1.0,
};

/// Behavior for the ender pearl item.
#[item_behavior(class = "EnderpearlItem")]
pub struct EnderPearlItem;

impl ItemBehavior for EnderPearlItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let world = context.world;
        let pearl = throw_item_projectile(context, &THROW, |spawn_pos| {
            EnderPearlEntity::new(
                &vanilla_entities::ENDER_PEARL,
                next_entity_id(),
                spawn_pos,
                Arc::downgrade(world),
            )
        });
        if let Some(pearl) = pearl {
            context.player.register_ender_pearl(&pearl);
        }
        InteractionResult::Success
    }
}
