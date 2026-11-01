//! Host side of world visits, time, weather, item drops, entities and extension messaging.
use super::{items::to_stack, players::find};
use crate::{PlayerId, Position, World, items::ItemSpec, worlds::Weather};
use glam::DVec3;
use std::sync::Arc;
use steel_core::{
    entity::{Entity, RemovalReason, next_entity_id},
    server::Server as CoreServer,
    world::World as CoreWorld,
};
use steel_protocol::packets::{
    common::CCustomPayload,
    game::{CAddEntity, SoundSource},
};
use steel_registry::{RegistryEntry, sound_events, vanilla_entities};
use steel_utils::Identifier;

/// Vanilla `LightningBolt` thunder is heard from 10 000 blocks; the impact from nearby.
const THUNDER_VOLUME: f32 = 10_000.0;
/// Vanilla `ServerLevel` lightning visibility for the add-entity packet.
const LIGHTNING_VIEW_DISTANCE_SQ: f64 = 512.0 * 512.0;

fn key(text: &str) -> Result<Identifier, String> {
    text.parse().map_err(|error: &str| error.to_owned())
}

const fn dvec(position: Position) -> DVec3 {
    DVec3::new(position.x, position.y, position.z)
}

pub(crate) fn visit_world(
    server: &CoreServer,
    name: &str,
    callback: extern "C" fn(World<'_>, u64),
    token: u64,
) -> Result<(), String> {
    let world = server
        .worlds
        .get(&key(name)?)
        .ok_or_else(|| format!("world {name} is not loaded"))?;
    callback(World::from_host(&world), token);
    Ok(())
}

pub(crate) fn send_plugin_message(
    server: &CoreServer,
    id: PlayerId,
    channel: &str,
    data: &[u8],
) -> Result<(), String> {
    let channel = key(channel)?;
    if channel.namespace == "minecraft" {
        return Err("extension channels must not use the minecraft namespace".into());
    }
    find(server, id)?.send_packet(CCustomPayload::new(channel, data.into()));
    Ok(())
}

pub(crate) fn day_time(world: &CoreWorld) -> i64 {
    world
        .dimension_type
        .default_clock
        .and_then(|clock| world.clock_total_ticks(clock))
        .unwrap_or(0)
}

pub(crate) fn set_day_time(world: &CoreWorld, ticks: i64) -> Result<(), String> {
    let clock = world
        .dimension_type
        .default_clock
        .ok_or("this dimension has no day clock")?;
    world
        .set_clock_total_ticks(clock, ticks)
        .ok_or_else(|| "this world does not track its day clock".to_owned())
}

pub(crate) fn weather(world: &CoreWorld) -> Weather {
    let level_data = world.level_data.read();
    match (level_data.is_raining(), level_data.is_thundering()) {
        (_, true) => Weather::Thunder,
        (true, false) => Weather::Rain,
        (false, false) => Weather::Clear,
    }
}

pub(crate) fn set_weather(
    world: &CoreWorld,
    weather: Weather,
    duration: i32,
) -> Result<(), String> {
    if duration <= 0 {
        return Err("weather duration must be positive".into());
    }
    // Same parameter sets as `/weather`.
    let (clear_time, rain_time, raining, thundering) = match weather {
        Weather::Clear => (duration, 0, false, false),
        Weather::Rain => (0, duration, true, false),
        Weather::Thunder => (0, duration, true, true),
    };
    world.set_weather_parameters(clear_time, rain_time, raining, thundering);
    Ok(())
}

pub(crate) fn drop_item(
    world: &Arc<CoreWorld>,
    position: Position,
    item: &ItemSpec,
) -> Result<i32, String> {
    let stack = to_stack(item)?;
    if stack.is_empty() {
        return Err("cannot drop an empty stack".into());
    }
    world
        .spawn_item(dvec(position), stack)
        .map(|entity| entity.id())
        .ok_or_else(|| "the item entity could not spawn".into())
}

pub(crate) fn remove_entity(world: &CoreWorld, entity_id: i32) -> Result<bool, String> {
    let Some(entity) = world.get_entity_by_id(entity_id) else {
        return Ok(false);
    };
    if entity.entity_type() == &vanilla_entities::PLAYER {
        return Err("players cannot be removed; kick or kill them instead".into());
    }
    entity.set_removed(RemovalReason::Discarded);
    Ok(true)
}

/// Mirrors Paper's `strikeLightningEffect`: a visual bolt the client removes by itself after
/// its flashes, with vanilla's thunder and impact sounds, but no fire or damage.
pub(crate) fn strike_lightning_effect(world: &CoreWorld, position: Position) -> i32 {
    let pos = dvec(position);
    #[expect(
        clippy::cast_possible_wrap,
        clippy::cast_possible_truncation,
        reason = "entity type ids are small registry indices"
    )]
    let packet = CAddEntity {
        id: next_entity_id(),
        uuid: uuid::Uuid::new_v4(),
        entity_type: vanilla_entities::LIGHTNING_BOLT.id() as i32,
        position: pos,
        velocity: DVec3::ZERO,
        x_rot: 0,
        y_rot: 0,
        head_y_rot: 0,
        data: 0,
    };
    let mut sent = 0;
    world.players.iter_players(|_, player| {
        if player.position().distance_squared(pos) < LIGHTNING_VIEW_DISTANCE_SQ {
            player.send_packet(packet.clone());
            sent += 1;
        }
        true
    });
    let pitch = || 0.8 + rand::random::<f32>() * 0.2;
    world.play_sound_at(
        &sound_events::ENTITY_LIGHTNING_BOLT_THUNDER,
        SoundSource::Weather,
        pos,
        THUNDER_VOLUME,
        pitch(),
        None,
    );
    world.play_sound_at(
        &sound_events::ENTITY_LIGHTNING_BOLT_IMPACT,
        SoundSource::Weather,
        pos,
        2.0,
        0.5 + rand::random::<f32>() * 0.2,
        None,
    );
    sent
}
