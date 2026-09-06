//! Egg item behavior (`EggItem`).
//!
//! Throwing an egg spawns a [`ThrownEggEntity`] from the player's eye, shot
//! along their look direction, and consumes one egg unless the player
//! has infinite materials. Mirrors vanilla `EggItem.use`.

use std::sync::Arc;

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::stat::vanilla_stat_types;
use steel_registry::{sound_events, vanilla_entities};

use crate::behavior::context::{InteractionResult, UseItemContext};
use crate::behavior::item::ItemBehavior;
use crate::entity::entities::ThrownEggEntity;
use crate::entity::{Entity, next_entity_id, spawn_throwable_item_projectile};

/// Vanilla `EggItem.PROJECTILE_SHOOT_POWER`.
const SHOOT_POWER: f32 = 1.5;
/// Vanilla `EggItem.use` throw sound volume.
const THROW_SOUND_VOLUME: f32 = 0.5;
/// Vanilla `EggItem.use` throw pitch jitter scale: `0.4 / (random * 0.4 + 0.8)`.
const THROW_PITCH_JITTER_SCALE: f32 = 0.4;
/// Vanilla `EggItem.use` throw pitch jitter base.
const THROW_PITCH_JITTER_BASE: f32 = 0.8;
/// Vanilla `EggItem.use` throw uncertainty (`spawnProjectileFromRotation`).
const THROW_UNCERTAINTY: f32 = 1.0;

/// Behavior for the egg item.
#[item_behavior(class = "EggItem")]
pub struct EggItem;

impl ItemBehavior for EggItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let player = context.player;
        let world = context.world;

        let pitch = THROW_PITCH_JITTER_SCALE
            / (rand::random::<f32>() * THROW_PITCH_JITTER_SCALE + THROW_PITCH_JITTER_BASE);
        world.play_sound_at(
            &sound_events::ENTITY_EGG_THROW,
            SoundSource::Players,
            player.position(),
            THROW_SOUND_VOLUME,
            pitch,
            None,
        );

        let mut thrown_item = context.inv.with_item(|item| item.clone());
        let Some(_egg) = spawn_throwable_item_projectile(
            world,
            player,
            &mut thrown_item,
            SHOOT_POWER,
            THROW_UNCERTAINTY,
            |spawn_pos| {
                ThrownEggEntity::new(
                    &vanilla_entities::EGG,
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
