//! Snowball item behavior (`SnowballItem`).
//!
//! Throwing a snowball spawns a [`SnowballEntity`] from the player's eye, shot
//! along their look direction, and consumes one snowball unless the player
//! has infinite materials. Mirrors vanilla `SnowballItem.use`.

use std::sync::Arc;

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::stat::vanilla_stat_types;
use steel_registry::{sound_events, vanilla_entities};

use crate::behavior::context::{InteractionResult, UseItemContext};
use crate::behavior::item::ItemBehavior;
use crate::entity::entities::SnowballEntity;
use crate::entity::{Entity, next_entity_id, spawn_throwable_item_projectile};

/// Vanilla `SnowballItem.PROJECTILE_SHOOT_POWER`.
const SHOOT_POWER: f32 = 1.5;
/// Vanilla `SnowballItem.use` throw sound volume.
const THROW_SOUND_VOLUME: f32 = 0.5;
/// Vanilla `SnowballItem.use` throw pitch jitter scale: `0.4 / (random * 0.4 + 0.8)`.
const THROW_PITCH_JITTER_SCALE: f32 = 0.4;
/// Vanilla `SnowballItem.use` throw pitch jitter base.
const THROW_PITCH_JITTER_BASE: f32 = 0.8;
/// Vanilla `SnowballItem.use` throw uncertainty (`spawnProjectileFromRotation`).
const THROW_UNCERTAINTY: f32 = 1.0;

/// Behavior for the snowball item.
#[item_behavior(class = "SnowballItem")]
pub struct SnowballItem;

impl ItemBehavior for SnowballItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let player = context.player;
        let world = context.world;

        let pitch = THROW_PITCH_JITTER_SCALE
            / (rand::random::<f32>() * THROW_PITCH_JITTER_SCALE + THROW_PITCH_JITTER_BASE);
        world.play_sound_at(
            &sound_events::ENTITY_SNOWBALL_THROW,
            SoundSource::Neutral,
            player.position(),
            THROW_SOUND_VOLUME,
            pitch,
            None,
        );

        let mut thrown_item = context.inv.with_item(|item| item.clone());
        let Some(_snowball) = spawn_throwable_item_projectile(
            world,
            player,
            &mut thrown_item,
            SHOOT_POWER,
            THROW_UNCERTAINTY,
            |spawn_pos| {
                SnowballEntity::new(
                    &vanilla_entities::SNOWBALL,
                    next_entity_id(),
                    spawn_pos,
                    Arc::downgrade(world),
                )
            },
        ) else {
            return InteractionResult::Fail;
        };

        player.award_stat(&vanilla_stat_types::ITEM_USED, thrown_item.item);
        let has_infinite_materials = player.has_infinite_materials();
        context
            .inv
            .with_item(|item| item.consume_one(has_infinite_materials));

        InteractionResult::Success
    }
}
