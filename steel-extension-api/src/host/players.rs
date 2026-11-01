//! Host implementations of player bindings.
use super::events::{parse_text, player_id};
use crate::{
    ApiResult, GameMode, PlayerId,
    events::Location,
    players::{MetadataKind, MetadataValue, PermissionValue},
};
use glam::DVec3;
use stabby::{string::String as ApiString, vec::Vec as ApiVec};
use std::sync::Arc;
use steel_core::{
    entity::{Entity, LivingEntity},
    extension::events::TeleportCause,
    inventory::container::Container,
    permission::{PermissionExpr, PermissionKey, PermissionMetadataValue, PermissionState},
    player::connection::NetworkConnection,
    player::{Player, player_inventory::Prediction},
    server::Server as CoreServer,
};
use steel_registry::{
    REGISTRY, RegistryExt, data_components::vanilla_components::CUSTOM_NAME, item_stack::ItemStack,
};
use steel_utils::{Identifier, types::GameType};

/// Finds the online player with this exact session identity.
pub(crate) fn find(server: &CoreServer, id: PlayerId) -> Result<Arc<Player>, String> {
    server
        .get_players()
        .into_iter()
        .find(|p| {
            p.gameprofile.id().as_bytes() == &id.bytes
                && p.id() == id.entity_id
                && !p.connection.closed()
        })
        .ok_or_else(|| "player is no longer connected".into())
}

/// Runs `read` on a connected player.
pub(crate) fn with_player<T>(
    server: &CoreServer,
    id: PlayerId,
    read: impl FnOnce(&Arc<Player>) -> T,
) -> Result<T, String> {
    find(server, id).map(|player| read(&player))
}

pub(crate) fn by_name(server: &CoreServer, name: &str) -> Option<PlayerId> {
    server
        .get_players()
        .into_iter()
        .find(|p| !p.connection.closed() && p.gameprofile.name().eq_ignore_ascii_case(name))
        .map(|p| player_id(&p))
}

pub(crate) fn by_uuid(server: &CoreServer, uuid: [u8; 16]) -> Option<PlayerId> {
    server
        .get_players()
        .into_iter()
        .find(|p| !p.connection.closed() && p.gameprofile.id().as_bytes() == &uuid)
        .map(|p| player_id(&p))
}

pub(crate) fn teleport_to(
    server: &CoreServer,
    id: PlayerId,
    destination: &Location,
) -> Result<(), String> {
    let player = find(server, id)?;
    let key: Identifier = destination.world.as_str().parse().map_err(str::to_owned)?;
    let world = server.worlds.get(&key).ok_or("world is not loaded")?;
    if world.domain() != player.get_world().domain() {
        return Err("teleport destinations must stay in the player's domain".into());
    }
    let position = DVec3::new(destination.x, destination.y, destination.z);
    if !position.is_finite() || !destination.yaw.is_finite() || !destination.pitch.is_finite() {
        return Err("teleport destination must be finite".into());
    }
    if !player.teleport_to(
        &world,
        position,
        (destination.yaw, destination.pitch),
        &TeleportCause::PLUGIN,
    ) {
        return Err("teleport was cancelled or could not be applied".into());
    }
    Ok(())
}

pub(crate) fn world_keys(server: &CoreServer) -> ApiVec<ApiString> {
    server
        .worlds
        .values()
        .map(|world| world.key.to_string().into())
        .collect()
}

pub(crate) fn health(server: &CoreServer, id: PlayerId) -> ApiResult<f32> {
    with_player(server, id, |p| p.get_health())
        .map_err(ApiString::from)
        .into()
}

pub(crate) fn max_health(server: &CoreServer, id: PlayerId) -> ApiResult<f32> {
    with_player(server, id, |p| p.get_max_health())
        .map_err(ApiString::from)
        .into()
}

pub(crate) fn set_health(server: &CoreServer, id: PlayerId, health: f32) -> Result<(), String> {
    if !health.is_finite() {
        return Err("health must be finite".into());
    }
    find(server, id)?.set_health(health);
    Ok(())
}

pub(crate) fn set_food(
    server: &CoreServer,
    id: PlayerId,
    food: i32,
    saturation: f32,
) -> Result<(), String> {
    if !saturation.is_finite() {
        return Err("saturation must be finite".into());
    }
    let player = find(server, id)?;
    let mut data = player.food_data.lock();
    data.food_level = food.clamp(0, 20);
    #[expect(
        clippy::cast_precision_loss,
        reason = "food levels are at most 20, exactly representable"
    )]
    let limit = data.food_level as f32;
    data.saturation_level = saturation.clamp(0.0, limit);
    Ok(())
}

pub(crate) fn game_mode(server: &CoreServer, id: PlayerId) -> ApiResult<GameMode> {
    with_player(server, id, |p| match p.game_mode() {
        GameType::Creative => GameMode::Creative,
        GameType::Adventure => GameMode::Adventure,
        GameType::Spectator => GameMode::Spectator,
        GameType::Survival => GameMode::Survival,
    })
    .map_err(ApiString::from)
    .into()
}

pub(crate) fn set_allow_flight(
    server: &CoreServer,
    id: PlayerId,
    allow: bool,
) -> Result<(), String> {
    let player = find(server, id)?;
    {
        let mut abilities = player.abilities.lock();
        abilities.may_fly = allow;
        if !allow {
            abilities.flying = false;
        }
    }
    player.send_abilities();
    Ok(())
}

pub(crate) fn kick(server: &CoreServer, id: PlayerId, reason: &str) -> Result<(), String> {
    find(server, id)?.disconnect(parse_text(reason));
    Ok(())
}

pub(crate) fn send_message(server: &CoreServer, id: PlayerId, text: &str) -> Result<(), String> {
    find(server, id)?.send_message(&parse_text(text));
    Ok(())
}

pub(crate) fn send_action_bar(server: &CoreServer, id: PlayerId, text: &str) -> Result<(), String> {
    find(server, id)?.send_action_bar(parse_text(text));
    Ok(())
}

pub(crate) fn send_title(
    server: &CoreServer,
    id: PlayerId,
    title: &str,
    subtitle: &str,
    (fade_in, stay, fade_out): (i32, i32, i32),
) -> Result<(), String> {
    let player = find(server, id)?;
    player.send_title_times(fade_in, stay, fade_out);
    if !subtitle.is_empty() {
        player.send_subtitle(parse_text(subtitle));
    }
    player.send_title(parse_text(title));
    Ok(())
}

pub(crate) fn broadcast(server: &CoreServer, text: &str) {
    server.broadcast_system_chat(&parse_text(text), None);
}

pub(crate) fn permission(
    server: &CoreServer,
    id: PlayerId,
    node: &str,
) -> Result<PermissionValue, String> {
    let key = PermissionKey::parse(node).map_err(|error| error.to_string())?;
    let player = find(server, id)?;
    Ok(match player.permission_state(&PermissionExpr::key(key)) {
        Some(PermissionState::Allow) => PermissionValue::Allow,
        Some(PermissionState::Deny) => PermissionValue::Deny,
        None => PermissionValue::Unset,
    })
}

pub(crate) fn permission_metadata(
    server: &CoreServer,
    id: PlayerId,
    key: &str,
) -> Result<Option<MetadataValue>, String> {
    let key: Identifier = key.parse().map_err(str::to_owned)?;
    let player = find(server, id)?;
    Ok(player.permission_metadata(&key).map(|value| {
        let text = value.to_string().into();
        match value {
            PermissionMetadataValue::Bool(boolean) => MetadataValue {
                kind: MetadataKind::Bool,
                text,
                integer: 0,
                boolean,
            },
            PermissionMetadataValue::Integer(integer) => MetadataValue {
                kind: MetadataKind::Integer,
                text,
                integer,
                boolean: false,
            },
            PermissionMetadataValue::String(_) => MetadataValue {
                kind: MetadataKind::String,
                text,
                integer: 0,
                boolean: false,
            },
        }
    }))
}

pub(crate) fn give_item(
    server: &CoreServer,
    id: PlayerId,
    item: &str,
    count: i32,
    name: &str,
) -> Result<i32, String> {
    if count <= 0 {
        return Err("item count must be positive".into());
    }
    let key: Identifier = item.parse().map_err(str::to_owned)?;
    let item = REGISTRY
        .items
        .by_key(&key)
        .ok_or_else(|| format!("unknown item {key}"))?;
    let player = find(server, id)?;
    let max_stack = ItemStack::new(item).max_stack_size().max(1);
    let mut remaining = count;
    let mut dropped = 0;
    while remaining > 0 {
        let size = remaining.min(max_stack);
        remaining -= size;
        let mut stack = ItemStack::with_count(item, size);
        if !name.is_empty() {
            stack.set(CUSTOM_NAME, parse_text(name));
        }
        let added = player.inventory.lock().add(&mut stack);
        if !added || !stack.is_empty() {
            dropped += stack.count();
            let _ = player.drop_item(stack, false, Prediction::ServerOnly);
        }
    }
    player.broadcast_inventory_changes();
    Ok(dropped)
}
