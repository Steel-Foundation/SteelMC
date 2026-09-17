//! Snowball item behavior (`SnowballItem`).
//!
//! Throwing a snowball spawns a [`SnowballEntity`] from the player's eye, shot
//! along their look direction, and consumes one snowball unless the player
//! has infinite materials. Mirrors vanilla `SnowballItem.use`.

use std::sync::Arc;

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::{sound_events, vanilla_entities};

use super::throw_projectile::{ThrowParams, throw_item_projectile};
use crate::behavior::context::{InteractionResult, UseItemContext};
use crate::behavior::item::ItemBehavior;
use crate::entity::entities::SnowballEntity;
use crate::entity::next_entity_id;

/// Vanilla `SnowballItem.use`'s sound, `PROJECTILE_SHOOT_POWER` and throw spread.
const THROW: ThrowParams = ThrowParams {
    sound: &sound_events::ENTITY_SNOWBALL_THROW,
    sound_source: SoundSource::Neutral,
    sound_volume: 0.5,
    y_offset: 0.0,
    power: 1.5,
    uncertainty: 1.0,
};

/// Behavior for the snowball item.
#[item_behavior(class = "SnowballItem")]
pub struct SnowballItem;

impl ItemBehavior for SnowballItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let world = context.world;
        throw_item_projectile(context, &THROW, |spawn_pos| {
            SnowballEntity::new(
                &vanilla_entities::SNOWBALL,
                next_entity_id(),
                spawn_pos,
                Arc::downgrade(world),
            )
        });
        InteractionResult::Success
    }
}
