//! Egg item behavior (`EggItem`).
//!
//! Throwing an egg spawns a [`ThrownEggEntity`] from the player's eye, shot
//! along their look direction, and consumes one egg (creative-mode count
//! restoration is handled by the caller). Mirrors vanilla `EggItem.use`.

use std::sync::Arc;

use glam::DVec3;
use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::{sound_events, vanilla_entities};

use crate::behavior::context::{InteractionResult, UseItemContext};
use crate::behavior::item::ItemBehavior;
use crate::entity::entities::ThrownEggEntity;
use crate::entity::{Entity, Projectile, SharedEntity, ThrowableItemProjectile, next_entity_id};

/// Vanilla `EggItem.PROJECTILE_SHOOT_POWER`.
const SHOOT_POWER: f32 = 1.5;
/// Vanilla `EggItem.use` throw sound volume.
const THROW_SOUND_VOLUME: f32 = 0.5;
/// Vanilla `EggItem.use` throw pitch jitter scale: `0.4 / (random * 0.4 + 0.8)`.
const THROW_PITCH_JITTER_SCALE: f32 = 0.4;
/// Vanilla `EggItem.use` throw pitch jitter base.
const THROW_PITCH_JITTER_BASE: f32 = 0.8;
/// Vanilla `ThrowableItemProjectile` spawn offset below the shooter's eye.
const THROWN_ITEM_EYE_OFFSET: f64 = 0.1;

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

        let thrown_item = context.inv.with_item(|item| item.clone());

        // Vanilla `ThrowableItemProjectile` spawns at the shooter's eye minus 0.1.
        let player_pos = player.position();
        let spawn_pos = DVec3::new(
            player_pos.x,
            player.get_eye_y() - THROWN_ITEM_EYE_OFFSET,
            player_pos.z,
        );

        let egg = Arc::new(ThrownEggEntity::new(
            &vanilla_entities::EGG,
            next_entity_id(),
            spawn_pos,
            Arc::downgrade(world),
        ));
        if let Some(owner) = world.players.get_by_uuid(&player.gameprofile.id) {
            let owner: SharedEntity = owner;
            egg.set_owner_entity(Some(&owner));
        } else {
            egg.set_owner_uuid(Some(player.gameprofile.id));
        }
        egg.set_item_clamped(thrown_item);

        let (yaw, player_pitch) = player.rotation();
        egg.shoot_from_rotation(player, player_pitch, yaw, 0.0, SHOOT_POWER, 1.0);

        let entity: SharedEntity = egg;
        if let Err(error) = world.try_add_entity(entity.clone()) {
            log::debug!("failed to spawn thrown egg: {error}");
            return InteractionResult::Fail;
        }

        // TODO: award the ITEM_USED stat once a stats system exists.
        context.inv.with_item(|item| item.shrink(1));

        InteractionResult::Success
    }
}
