//! Wind charge item behavior (`WindChargeItem`).
//!
//! Throwing a wind charge spawns a [`WindChargeEntity`] at the player's eye,
//! shot along their look direction, and consumes one charge unless the player
//! has infinite materials. The item's `use_cooldown` component is applied by
//! the shared item-use path.

use std::sync::Arc;

use glam::DVec3;
use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::{sound_events, vanilla_entities};

use super::throw_projectile::{ThrowParams, throw_projectile};
use crate::behavior::context::{InteractionResult, UseItemContext};
use crate::behavior::item::ItemBehavior;
use crate::entity::entities::WindChargeEntity;
use crate::entity::spawn_projectile_from_rotation;
use crate::entity::{Entity as _, Projectile as _, SharedEntity, next_entity_id};
use crate::player::Player;

/// Sound, shoot power and spread used when throwing a wind charge.
const THROW: ThrowParams = ThrowParams {
    sound: &sound_events::ENTITY_WIND_CHARGE_THROW,
    sound_source: SoundSource::Neutral,
    sound_volume: 0.5,
    y_offset: 0.0,
    power: 1.5,
    uncertainty: 1.0,
};

/// Behavior for the wind charge item.
// TODO: Implement the `ProjectileItem` dispenser dispatch.
#[item_behavior(class = "WindChargeItem")]
pub struct WindChargeItem;

impl ItemBehavior for WindChargeItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        throw_projectile(context, &THROW, |world, player, thrown_item| {
            let position = player.position();
            let wind_charge = WindChargeEntity::new(
                &vanilla_entities::WIND_CHARGE,
                next_entity_id(),
                DVec3::new(position.x, player.get_eye_y(), position.z),
                Arc::downgrade(world),
            );
            let owner: SharedEntity = Arc::<Player>::clone(player);
            wind_charge.set_owner_entity(Some(&owner));
            spawn_projectile_from_rotation(
                world,
                player,
                thrown_item,
                THROW.y_offset,
                THROW.power,
                THROW.uncertainty,
                wind_charge,
            )
        });
        InteractionResult::Success
    }
}
