//! Bottle o' Enchanting item behavior (`ExperienceBottleItem`).
//!
//! Throwing a bottle spawns a [`ThrownExperienceBottleEntity`] from the
//! player's eye, shot 20 degrees above their look direction, and consumes one
//! bottle unless the player has infinite materials. Mirrors vanilla
//! `ExperienceBottleItem.use`. Dispenser support waits for a dispenser.

use std::sync::Arc;

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::stat::vanilla_stat_types;
use steel_registry::{sound_events, vanilla_entities};

use crate::behavior::context::{InteractionResult, UseItemContext};
use crate::behavior::item::ItemBehavior;
use crate::entity::entities::ThrownExperienceBottleEntity;
use crate::entity::{Entity, next_entity_id, spawn_throwable_item_projectile};

/// Vanilla `ExperienceBottleItem.use` pitch offset in degrees (`spawnProjectileFromRotation` z argument).
const PITCH_OFFSET: f32 = -20.0;
/// Vanilla `ExperienceBottleItem.use` shoot power.
const SHOOT_POWER: f32 = 0.7;
/// Vanilla `ExperienceBottleItem.use` throw uncertainty.
const THROW_UNCERTAINTY: f32 = 1.0;
/// Vanilla `ExperienceBottleItem.use` throw sound volume.
const THROW_SOUND_VOLUME: f32 = 0.5;
/// Vanilla throw pitch jitter scale: `0.4 / (random * 0.4 + 0.8)`.
const THROW_PITCH_JITTER_SCALE: f32 = 0.4;
/// Vanilla throw pitch jitter base.
const THROW_PITCH_JITTER_BASE: f32 = 0.8;

/// Behavior for the Bottle o' Enchanting item.
#[item_behavior(class = "ExperienceBottleItem")]
pub struct ExperienceBottleItem;

impl ItemBehavior for ExperienceBottleItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let player = context.player;
        let world = context.world;

        let pitch = THROW_PITCH_JITTER_SCALE
            / rand::random_range(
                THROW_PITCH_JITTER_BASE..THROW_PITCH_JITTER_BASE + THROW_PITCH_JITTER_SCALE,
            );
        world.play_sound_at(
            &sound_events::ENTITY_EXPERIENCE_BOTTLE_THROW,
            SoundSource::Neutral,
            player.position(),
            THROW_SOUND_VOLUME,
            pitch,
            None,
        );

        let mut thrown_item = context.inv.with_item(|item| item.clone());
        let Some(_bottle) = spawn_throwable_item_projectile(
            world,
            player,
            &mut thrown_item,
            PITCH_OFFSET,
            SHOOT_POWER,
            THROW_UNCERTAINTY,
            |spawn_pos| {
                ThrownExperienceBottleEntity::new(
                    &vanilla_entities::EXPERIENCE_BOTTLE,
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
