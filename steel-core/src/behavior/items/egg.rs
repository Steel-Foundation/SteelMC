//! Egg item behavior (`EggItem`).
//!
//! Throwing an egg spawns a [`ThrownEggEntity`] from the player's eye, shot
//! along their look direction, and consumes one egg unless the player
//! has infinite materials. Mirrors vanilla `EggItem.use`.

use std::sync::Arc;

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::{sound_events, vanilla_entities};

use super::throw_projectile::{ThrowParams, throw_item_projectile};
use crate::behavior::context::{InteractionResult, UseItemContext};
use crate::behavior::item::ItemBehavior;
use crate::entity::entities::ThrownEggEntity;
use crate::entity::next_entity_id;

/// Vanilla `EggItem.use`'s sound, `PROJECTILE_SHOOT_POWER` and throw spread.
const THROW: ThrowParams = ThrowParams {
    sound: &sound_events::ENTITY_EGG_THROW,
    sound_source: SoundSource::Players,
    sound_volume: 0.5,
    pitch_offset: 0.0,
    power: 1.5,
    uncertainty: 1.0,
};

/// Behavior for the egg item.
#[item_behavior(class = "EggItem")]
pub struct EggItem;

impl ItemBehavior for EggItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let world = context.world;
        throw_item_projectile(context, &THROW, |spawn_pos| {
            ThrownEggEntity::new(
                &vanilla_entities::EGG,
                next_entity_id(),
                spawn_pos,
                Arc::downgrade(world),
            )
        });
        InteractionResult::Success
    }
}
