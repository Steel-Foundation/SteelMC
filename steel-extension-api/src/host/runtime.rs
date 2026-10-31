//! The native command and runtime boundary. Gameplay continues to use core paths.
use super::HookRegistration;
use crate::{
    ApiResult, BlockPos, BlockState,
    runtime::{
        BotInput, GameMode, Operation, OperationState, OperationStatus, PlayerId, PlayerInfo,
        Position, WorldSettings,
        api::{Command, Commands, Server},
    },
};
use stabby::{str::Str, string::String as ApiString, vec::Vec as ApiVec};
use std::collections::BTreeSet;
use std::sync::Arc;
use steel_core::{
    command::{CommandError, CommandNode, CommandRegistration, CommandRegistry, literal},
    entity::{Entity, LivingEntity},
    extension::ServerTickHandler,
    inventory::container::Container,
    player::{Player, PlayerInput, bot::BotInput as CoreBotInput, connection::NetworkConnection},
    server::{
        Server as CoreServer,
        temporary::{TemporaryWorldRequest, WorldOperationId},
    },
};
use steel_registry::{REGISTRY, RegistryExt, item_stack::ItemStack};
use steel_utils::Identifier;
use toml::{Value, map::Map};

pub(crate) struct Tick(pub extern "C" fn(Server<'_>, u64));
impl ServerTickHandler for Tick {
    fn tick(&self, server: &Arc<CoreServer>, tick: u64) {
        (self.0)(Server::from_host(server), tick);
    }
}
pub(crate) fn subscribe_commands(
    hooks: &mut HookRegistration<'_>,
    callback: extern "C" fn(Commands<'_>) -> ApiResult<()>,
) -> Result<(), String> {
    hooks.extension.begin_registration();
    let extension = Arc::clone(hooks.extension);
    let result = CommandRegistry::on_registration(move |registry| {
        extension.run_registration("commands", || callback(Commands::from_host(registry)));
    });
    if result.is_err() {
        hooks.extension.cancel_registration();
    }
    result
}
pub(crate) fn register_command(
    registry: &mut CommandRegistry,
    key: &str,
    actions: &[Str<'_>],
    callback: extern "C" fn(Command<'_>, Str<'_>) -> ApiResult<i32>,
) -> Result<(), String> {
    let key: Identifier = key.parse().map_err(str::to_owned)?;
    if actions.is_empty() {
        return Err("command requires at least one subcommand".into());
    }
    let mut node: CommandNode =
        literal(key.path.to_string()).requires(|source| source.player().is_some());
    let mut names = BTreeSet::new();
    for action in actions {
        if action.as_str().is_empty()
            || action.as_str().chars().any(char::is_whitespace)
            || !names.insert(action.as_str())
        {
            return Err("invalid or duplicate command subcommand".into());
        }
        let action = action.as_str().to_owned();
        node = node.then(literal(action.clone()).executes(move |context| {
            let result: Result<i32, ApiString> =
                callback(Command::from_host(context), Str::new(&action)).into();
            result.map_err(|error| CommandError::from(error.to_string()))
        }));
    }
    registry
        .register(CommandRegistration::new(key, move || node).default_access())
        .map_err(|e| e.to_string())?;
    Ok(())
}
fn find_player(server: &CoreServer, id: PlayerId) -> Result<Arc<Player>, String> {
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
fn key(value: &str) -> Result<Identifier, String> {
    value.parse().map_err(str::to_owned)
}
const fn position(value: Position) -> glam::DVec3 {
    glam::DVec3::new(value.x, value.y, value.z)
}
pub(crate) fn create_world(
    server: &Arc<CoreServer>,
    name: &str,
    generator: &str,
    settings: WorldSettings,
    primary: bool,
) -> Result<Operation, String> {
    server
        .create_temporary_world(TemporaryWorldRequest {
            key: key(name)?,
            primary,
            generator: key(generator)?,
            config: Value::Table(Map::new()),
            seed: settings.seed,
            spawn: super::position(settings.spawn),
            default_gamemode: settings.default_game_mode.into_host(),
            difficulty: settings.difficulty.into_host(),
        })
        .map(|id| Operation { id: id.0 })
}
pub(crate) fn operation_status(
    server: &CoreServer,
    operation: Operation,
) -> Result<OperationStatus, String> {
    let (state, value) = match server.take_world_operation(WorldOperationId(operation.id))? {
        None => (OperationState::Pending, ApiString::from("")),
        Some(Ok(key)) => (OperationState::Ready, key.to_string().into()),
        Some(Err(error)) => (OperationState::Failed, error.into()),
    };
    Ok(OperationStatus { value, state })
}
pub(crate) fn players(server: &CoreServer) -> ApiVec<PlayerInfo> {
    server
        .get_players()
        .into_iter()
        .filter(|p| {
            !p.connection.closed()
                && p.get_world()
                    .players
                    .get_by_uuid(&p.gameprofile.id())
                    .is_some_and(|live| Arc::ptr_eq(&live, p))
        })
        .map(|p| {
            let pos = p.position();
            PlayerInfo {
                id: PlayerId {
                    bytes: *p.gameprofile.id().as_bytes(),
                    entity_id: p.id(),
                },
                name: p.gameprofile.name().into(),
                world: p.get_world().key.to_string().into(),
                position: Position {
                    x: pos.x,
                    y: pos.y,
                    z: pos.z,
                },
                bot: p.is_server_controlled(),
                health: p.get_health(),
            }
        })
        .collect()
}
pub(crate) fn switch_domain(server: &CoreServer, id: PlayerId, domain: &str) -> Result<(), String> {
    server.queue_domain_switch(find_player(server, id)?, domain.to_owned())
}
pub(crate) fn select_world(server: &CoreServer, id: PlayerId, name: &str) -> Result<(), String> {
    let world = server
        .worlds
        .get(&key(name)?)
        .ok_or("world is not loaded")?;
    server.queue_player_world_selection(find_player(server, id)?, &world)
}
pub(crate) fn teleport(server: &CoreServer, id: PlayerId, pos: Position) -> Result<(), String> {
    let player = find_player(server, id)?;
    if !position(pos).is_finite() {
        return Err("position must be finite".into());
    }
    let target = BlockPos {
        x: pos.x.floor() as i32,
        y: pos.y.floor() as i32,
        z: pos.z.floor() as i32,
    };
    super::read_block(&player.get_world(), target)?;
    player
        .teleport(position(pos), 0.0, 0.0)
        .map_err(|e| e.to_string())
}
pub(crate) fn set_mode(server: &CoreServer, id: PlayerId, mode: GameMode) -> Result<(), String> {
    find_player(server, id)?.set_game_mode(mode.into_host());
    Ok(())
}
pub(crate) fn clear_inventory(server: &CoreServer, id: PlayerId) -> Result<(), String> {
    find_player(server, id)?.inventory.lock().clear_content();
    Ok(())
}
pub(crate) fn equip_item(server: &CoreServer, id: PlayerId, name: &str) -> Result<(), String> {
    let item = REGISTRY.items.by_key(&key(name)?).ok_or("unknown item")?;
    find_player(server, id)?
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(item));
    Ok(())
}
pub(crate) fn message(server: &CoreServer, id: PlayerId, text: &str) -> Result<(), String> {
    find_player(server, id)?.send_message(&text_components::TextComponent::plain(text.to_owned()));
    Ok(())
}
pub(crate) fn spawn_bot(
    server: &Arc<CoreServer>,
    name: &str,
    player_name: &str,
    pos: Position,
) -> Result<PlayerId, String> {
    server
        .spawn_bot(&key(name)?, player_name, position(pos))
        .map(|p| PlayerId {
            bytes: *p.gameprofile.id().as_bytes(),
            entity_id: p.id(),
        })
}
pub(crate) fn despawn_bot(server: &CoreServer, id: PlayerId) -> Result<(), String> {
    server.despawn_bot(&find_player(server, id)?)
}
pub(crate) fn control_bot(
    server: &CoreServer,
    id: PlayerId,
    input: BotInput,
) -> Result<(), String> {
    let m = input.movement;
    find_player(server, id)?.control_bot(CoreBotInput {
        yaw: input.yaw,
        pitch: input.pitch,
        movement: PlayerInput::from_controls(
            m.forward, m.backward, m.left, m.right, m.jump, m.sneak, m.sprint,
        ),
        dig: Option::<BlockPos>::from(input.dig).map(super::position),
    })
}
pub(crate) fn block(server: &CoreServer, name: &str, pos: BlockPos) -> Result<BlockState, String> {
    let world = server
        .worlds
        .get(&key(name)?)
        .ok_or("world is not loaded")?;
    super::read_block(&world, pos)
}

pub(crate) fn is_connected(server: &CoreServer, id: PlayerId) -> bool {
    find_player(server, id).is_ok()
}
pub(crate) fn player_ready(server: &CoreServer, id: PlayerId) -> bool {
    find_player(server, id).is_ok_and(|p| {
        !p.is_world_change_pending()
            && !p.is_domain_switching()
            && p.has_client_loaded()
            && p.has_joined_world()
    })
}
