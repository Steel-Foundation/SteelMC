//! Ender pearl item behavior (`EnderpearlItem`).
//!
//! Throwing an ender pearl spawns a [`EnderPearlEntity`] from the player's eye,
//! shot along their look direction, and consumes one pearl (creative-mode count
//! restoration is handled by the caller). Mirrors vanilla `EnderpearlItem.use`.

use std::sync::Arc;

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::stat::vanilla_stat_types;
use steel_registry::{sound_events, vanilla_entities};

use crate::behavior::context::{InteractionResult, UseItemContext};
use crate::behavior::item::ItemBehavior;
use crate::entity::entities::EnderPearlEntity;
use crate::entity::{Entity, next_entity_id, spawn_throwable_item_projectile};

/// Vanilla `EnderpearlItem.PROJECTILE_SHOOT_POWER`.
const SHOOT_POWER: f32 = 1.5;
/// Vanilla `EnderpearlItem.use` throw uncertainty (`spawnProjectileFromRotation`).
const THROW_UNCERTAINTY: f32 = 1.0;

/// Behavior for the ender pearl item.
#[item_behavior(class = "EnderpearlItem")]
pub struct EnderPearlItem;

impl ItemBehavior for EnderPearlItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let player = context.player;
        let world = context.world;

        let pitch = 0.4 / (rand::random::<f32>() * 0.4 + 0.8);
        world.play_sound_at(
            &sound_events::ENTITY_ENDER_PEARL_THROW,
            SoundSource::Neutral,
            player.position(),
            0.5,
            pitch,
            None,
        );

        let mut thrown_item = context.inv.with_item(|item| item.clone());
        let Some(entity) = spawn_throwable_item_projectile(
            world,
            player,
            &mut thrown_item,
            SHOOT_POWER,
            THROW_UNCERTAINTY,
            |spawn_pos| {
                EnderPearlEntity::new(
                    &vanilla_entities::ENDER_PEARL,
                    next_entity_id(),
                    spawn_pos,
                    Arc::downgrade(world),
                )
            },
        ) else {
            return InteractionResult::Fail;
        };
        player.register_ender_pearl(&entity);

        player.award_stat(&vanilla_stat_types::ITEM_USED, thrown_item.item);
        if !player.has_infinite_materials() {
            context.inv.with_item(|item| item.shrink(1));
        }

        InteractionResult::Success
    }
}
