//! Shared item use logic for throwing projectiles

use glam::DVec3;
use steel_protocol::packets::game::SoundSource;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::stat::vanilla_stat_types;

use crate::behavior::context::UseItemContext;
use crate::entity::{
    Entity, Projectile, SharedEntity, ThrowableItemProjectile, spawn_throwable_item_projectile,
};

const THROW_PITCH_NUMERATOR: f32 = 0.4;
const THROW_PITCH_DIVISOR_MIN: f32 = 0.8;
const THROW_PITCH_DIVISOR_MAX: f32 = 1.2;

/// The per-item constants of vanilla's throw `use`.
pub(super) struct ThrowParams {
    /// Sound played before the projectile spawns.
    pub sound: SoundEventRef,
    /// Category that sound is played under.
    pub sound_source: SoundSource,
    /// Volume that sound is played at.
    pub sound_volume: f32,
    /// `spawnProjectileFromRotation`'s `yOffset`, in degrees.
    pub y_offset: f32,
    /// The item's `PROJECTILE_SHOOT_POWER`.
    pub power: f32,
    /// `spawnProjectileFromRotation` uncertainty.
    pub uncertainty: f32,
}

/// Throws a projectile plays its sound awards `ITEM_USED` and consumes one item
pub(super) fn throw_item_projectile<E>(
    context: &mut UseItemContext,
    params: &ThrowParams,
    create: impl FnOnce(DVec3) -> E,
) -> Option<SharedEntity>
where
    E: Projectile + ThrowableItemProjectile + Entity + 'static,
{
    let player = context.player;
    let world = context.world;

    let pitch = THROW_PITCH_NUMERATOR
        / rand::random_range(THROW_PITCH_DIVISOR_MIN..THROW_PITCH_DIVISOR_MAX);
    world.play_sound_at(
        params.sound,
        params.sound_source,
        player.position(),
        params.sound_volume,
        pitch,
        None,
    );

    let mut thrown_item = context.inv.with_item(|item| item.clone());
    let projectile = spawn_throwable_item_projectile(
        world,
        player,
        &mut thrown_item,
        params.y_offset,
        params.power,
        params.uncertainty,
        create,
    );
    if projectile.is_none() {
        log::debug!("world rejected a thrown projectile; still consuming the stack");
    }

    player.award_stat(&vanilla_stat_types::ITEM_USED, thrown_item.item);
    let has_infinite_materials = player.has_infinite_materials();
    context
        .inv
        .with_item(|item| item.consume_one(has_infinite_materials));

    projectile
}
