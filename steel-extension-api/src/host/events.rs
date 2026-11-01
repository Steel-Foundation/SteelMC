//! Core event handlers that call native extension callbacks.
use super::HookRegistration;
use crate::{
    BlockPos, PlayerId, Position,
    events::{
        EntityInfo, EventPriority, Face, Hand, InteractAction, Location,
        api::{
            BlockBreakEvent, BlockPlaceEvent, ChatEvent, CommandEvent, DamageEvent, DeathEvent,
            InteractEvent, JoinEvent, MoveEvent, PreLoginEvent, QuitEvent, TeleportEvent,
        },
    },
};
use glam::DVec3;
use stabby::{option::Option as StableOption, string::String as ApiString};
use std::sync::Arc;
use steel_core::{
    entity::Entity,
    extension::{
        EventHandler, EventPriority as CorePriority,
        events::{
            BlockBreakEvent as CoreBreak, BlockPlaceEvent as CorePlace, EntityDamageEvent,
            EntityInfo as CoreEntityInfo, InteractAction as CoreAction, PlayerChatEvent,
            PlayerCommandEvent, PlayerDeathEvent, PlayerInteractEvent, PlayerJoinEvent,
            PlayerMoveEvent, PlayerPreLoginEvent, PlayerQuitEvent, PlayerTeleportEvent,
        },
    },
    player::Player,
    server::Server as CoreServer,
    world::World,
};
use steel_utils::{Direction, Identifier, types::InteractionHand};
use text_components::TextComponent;

/// Mutable access to a player event, with the server that owns the player.
pub(crate) struct Access<'a, E> {
    pub event: &'a mut E,
    pub read_only: bool,
    server: &'a Arc<CoreServer>,
    world: &'a Arc<World>,
}

impl<'a, E> Access<'a, E> {
    pub(crate) const fn writable(&self) -> bool {
        !self.read_only
    }

    pub(crate) const fn server(&self) -> &'a Arc<CoreServer> {
        self.server
    }

    /// The player's world, where every player event happens.
    pub(crate) const fn world(&self) -> &'a Arc<World> {
        self.world
    }
}

/// Mutable access to an event without an owning player.
pub(crate) struct BareAccess<'a, E> {
    pub event: &'a mut E,
    pub read_only: bool,
}

impl<E> BareAccess<'_, E> {
    pub(crate) const fn writable(&self) -> bool {
        !self.read_only
    }
}

/// Mutable access to an event that happens in a world without an owning player.
pub(crate) struct WorldAccess<'a, E> {
    pub event: &'a mut E,
    pub read_only: bool,
    world: &'a Arc<World>,
}

impl<'a, E> WorldAccess<'a, E> {
    pub(crate) const fn writable(&self) -> bool {
        !self.read_only
    }

    pub(crate) const fn world(&self) -> &'a Arc<World> {
        self.world
    }
}

pub(crate) type PreLoginAccess<'a> = BareAccess<'a, PlayerPreLoginEvent>;
pub(crate) type JoinAccess<'a> = Access<'a, PlayerJoinEvent>;
pub(crate) type QuitAccess<'a> = Access<'a, PlayerQuitEvent>;
pub(crate) type ChatAccess<'a> = Access<'a, PlayerChatEvent>;
pub(crate) type CommandAccess<'a> = Access<'a, PlayerCommandEvent>;
pub(crate) type BreakAccess<'a> = Access<'a, CoreBreak>;
pub(crate) type PlaceAccess<'a> = Access<'a, CorePlace>;
pub(crate) type InteractAccess<'a> = Access<'a, PlayerInteractEvent>;
pub(crate) type DamageAccess<'a> = BareAccess<'a, EntityDamageEvent>;
pub(crate) type DeathAccess<'a> = Access<'a, PlayerDeathEvent>;
pub(crate) type MoveAccess<'a> = Access<'a, PlayerMoveEvent>;
pub(crate) type TeleportAccess<'a> = Access<'a, PlayerTeleportEvent>;

const fn core_priority(priority: EventPriority) -> CorePriority {
    match priority {
        EventPriority::Lowest => CorePriority::Lowest,
        EventPriority::Low => CorePriority::Low,
        EventPriority::Normal => CorePriority::Normal,
        EventPriority::High => CorePriority::High,
        EventPriority::Highest => CorePriority::Highest,
        EventPriority::Monitor => CorePriority::Monitor,
    }
}

/// Defines a handler for a player event whose server is reached through `event.player`.
macro_rules! player_handler {
    ($handler:ident, $subscribe:ident, $core:ty, $handle:ident) => {
        struct $handler {
            callback: extern "C" fn($handle<'_>, u64),
            token: u64,
        }

        impl EventHandler<$core> for $handler {
            fn handle(&self, event: &mut $core, priority: CorePriority) {
                let server = event.player.server();
                let world = event.player.get_world();
                let mut access = Access {
                    event,
                    read_only: priority == CorePriority::Monitor,
                    server: &server,
                    world: &world,
                };
                (self.callback)($handle::from_host(&mut access), self.token);
            }
        }

        pub(crate) fn $subscribe(
            hooks: &mut HookRegistration<'_>,
            priority: EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn($handle<'_>, u64),
            token: u64,
        ) {
            subscribe_to(
                hooks,
                |events| &mut events.$subscribe,
                priority,
                ignore_cancelled,
                Box::new($handler { callback, token }),
            );
        }
    };
}

/// Defines a handler for an event without an owning player.
macro_rules! bare_handler {
    ($handler:ident, $subscribe:ident, $core:ty, $handle:ident) => {
        struct $handler {
            callback: extern "C" fn($handle<'_>, u64),
            token: u64,
        }

        impl EventHandler<$core> for $handler {
            fn handle(&self, event: &mut $core, priority: CorePriority) {
                let mut access = BareAccess {
                    event,
                    read_only: priority == CorePriority::Monitor,
                };
                (self.callback)($handle::from_host(&mut access), self.token);
            }
        }

        pub(crate) fn $subscribe(
            hooks: &mut HookRegistration<'_>,
            priority: EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn($handle<'_>, u64),
            token: u64,
        ) {
            subscribe_to(
                hooks,
                |events| &mut events.$subscribe,
                priority,
                ignore_cancelled,
                Box::new($handler { callback, token }),
            );
        }
    };
}

/// Defines a handler for a world event whose world is `event.world`.
macro_rules! world_handler {
    ($handler:ident, $subscribe:ident, $core:ty, $handle:ident) => {
        struct $handler {
            callback: extern "C" fn($handle<'_>, u64),
            token: u64,
        }

        impl EventHandler<$core> for $handler {
            fn handle(&self, event: &mut $core, priority: CorePriority) {
                let world = Arc::clone(&event.world);
                let mut access = WorldAccess {
                    event,
                    read_only: priority == CorePriority::Monitor,
                    world: &world,
                };
                (self.callback)($handle::from_host(&mut access), self.token);
            }
        }

        pub(crate) fn $subscribe(
            hooks: &mut HookRegistration<'_>,
            priority: EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn($handle<'_>, u64),
            token: u64,
        ) {
            subscribe_to(
                hooks,
                |events| &mut events.$subscribe,
                priority,
                ignore_cancelled,
                Box::new($handler { callback, token }),
            );
        }
    };
}

fn subscribe_to<E: steel_core::extension::Event>(
    hooks: &mut HookRegistration<'_>,
    bus: impl FnOnce(&mut steel_core::extension::Events) -> &mut steel_core::extension::EventBus<E>,
    priority: EventPriority,
    ignore_cancelled: bool,
    handler: Box<dyn EventHandler<E>>,
) {
    bus(&mut hooks.hooks.events).subscribe(core_priority(priority), ignore_cancelled, handler);
}

// Per-event handlers; each generated function is named after its `Events` bus field.
bare_handler!(
    PreLoginHandler,
    pre_login,
    PlayerPreLoginEvent,
    PreLoginEvent
);
player_handler!(JoinHandler, join, PlayerJoinEvent, JoinEvent);
player_handler!(QuitHandler, quit, PlayerQuitEvent, QuitEvent);
player_handler!(ChatHandler, chat, PlayerChatEvent, ChatEvent);
player_handler!(CommandHandler, command, PlayerCommandEvent, CommandEvent);
player_handler!(BreakHandler, block_break, CoreBreak, BlockBreakEvent);
player_handler!(PlaceHandler, block_place, CorePlace, BlockPlaceEvent);
player_handler!(
    InteractHandler,
    interact,
    PlayerInteractEvent,
    InteractEvent
);
bare_handler!(DamageHandler, damage, EntityDamageEvent, DamageEvent);
player_handler!(DeathHandler, death, PlayerDeathEvent, DeathEvent);
player_handler!(MoveHandler, movement, PlayerMoveEvent, MoveEvent);
player_handler!(
    TeleportHandler,
    teleport,
    PlayerTeleportEvent,
    TeleportEvent
);

pub(crate) fn subscribe_pre_login(
    hooks: &mut HookRegistration<'_>,
    priority: EventPriority,
    ignore_cancelled: bool,
    callback: extern "C" fn(PreLoginEvent<'_>, u64),
    token: u64,
) {
    pre_login(hooks, priority, ignore_cancelled, callback, token);
}
pub(crate) fn subscribe_join(
    hooks: &mut HookRegistration<'_>,
    priority: EventPriority,
    callback: extern "C" fn(JoinEvent<'_>, u64),
    token: u64,
) {
    join(hooks, priority, false, callback, token);
}
pub(crate) fn subscribe_quit(
    hooks: &mut HookRegistration<'_>,
    priority: EventPriority,
    callback: extern "C" fn(QuitEvent<'_>, u64),
    token: u64,
) {
    quit(hooks, priority, false, callback, token);
}
pub(crate) fn subscribe_chat(
    hooks: &mut HookRegistration<'_>,
    priority: EventPriority,
    ignore_cancelled: bool,
    callback: extern "C" fn(ChatEvent<'_>, u64),
    token: u64,
) {
    chat(hooks, priority, ignore_cancelled, callback, token);
}
pub(crate) fn subscribe_command(
    hooks: &mut HookRegistration<'_>,
    priority: EventPriority,
    ignore_cancelled: bool,
    callback: extern "C" fn(CommandEvent<'_>, u64),
    token: u64,
) {
    command(hooks, priority, ignore_cancelled, callback, token);
}
pub(crate) fn subscribe_block_break(
    hooks: &mut HookRegistration<'_>,
    priority: EventPriority,
    ignore_cancelled: bool,
    callback: extern "C" fn(BlockBreakEvent<'_>, u64),
    token: u64,
) {
    block_break(hooks, priority, ignore_cancelled, callback, token);
}
pub(crate) fn subscribe_block_place(
    hooks: &mut HookRegistration<'_>,
    priority: EventPriority,
    ignore_cancelled: bool,
    callback: extern "C" fn(BlockPlaceEvent<'_>, u64),
    token: u64,
) {
    block_place(hooks, priority, ignore_cancelled, callback, token);
}
pub(crate) fn subscribe_interact(
    hooks: &mut HookRegistration<'_>,
    priority: EventPriority,
    ignore_cancelled: bool,
    callback: extern "C" fn(InteractEvent<'_>, u64),
    token: u64,
) {
    interact(hooks, priority, ignore_cancelled, callback, token);
}
pub(crate) fn subscribe_damage(
    hooks: &mut HookRegistration<'_>,
    priority: EventPriority,
    ignore_cancelled: bool,
    callback: extern "C" fn(DamageEvent<'_>, u64),
    token: u64,
) {
    damage(hooks, priority, ignore_cancelled, callback, token);
}
pub(crate) fn subscribe_death(
    hooks: &mut HookRegistration<'_>,
    priority: EventPriority,
    callback: extern "C" fn(DeathEvent<'_>, u64),
    token: u64,
) {
    death(hooks, priority, false, callback, token);
}
pub(crate) fn subscribe_move(
    hooks: &mut HookRegistration<'_>,
    priority: EventPriority,
    ignore_cancelled: bool,
    callback: extern "C" fn(MoveEvent<'_>, u64),
    token: u64,
) {
    movement(hooks, priority, ignore_cancelled, callback, token);
}
pub(crate) fn subscribe_teleport(
    hooks: &mut HookRegistration<'_>,
    priority: EventPriority,
    ignore_cancelled: bool,
    callback: extern "C" fn(TeleportEvent<'_>, u64),
    token: u64,
) {
    teleport(hooks, priority, ignore_cancelled, callback, token);
}

/// Parses vanilla JSON text when `text` starts with `{`, otherwise treats it as plain text.
/// Invalid JSON is shown literally rather than dropped.
pub(crate) fn parse_text(text: &str) -> TextComponent {
    if text.trim_start().starts_with('{')
        && let Ok(component) = serde_json::from_str::<TextComponent>(text)
    {
        return component;
    }
    TextComponent::plain(text.to_owned())
}

pub(crate) fn text_json(text: Option<&TextComponent>) -> StableOption<ApiString> {
    text.and_then(|text| serde_json::to_string(text).ok())
        .map(ApiString::from)
        .into()
}

pub(crate) fn player_id(player: &Player) -> PlayerId {
    PlayerId {
        bytes: *player.gameprofile.id().as_bytes(),
        entity_id: player.id(),
    }
}

pub(crate) const fn block_pos(pos: steel_utils::BlockPos) -> BlockPos {
    BlockPos {
        x: pos.x(),
        y: pos.y(),
        z: pos.z(),
    }
}

pub(crate) const fn position(pos: DVec3) -> Position {
    Position {
        x: pos.x,
        y: pos.y,
        z: pos.z,
    }
}

pub(crate) const fn face(direction: Direction) -> Face {
    match direction {
        Direction::Down => Face::Down,
        Direction::Up => Face::Up,
        Direction::North => Face::North,
        Direction::South => Face::South,
        Direction::West => Face::West,
        Direction::East => Face::East,
    }
}

pub(crate) const fn hand(hand: InteractionHand) -> Hand {
    match hand {
        InteractionHand::MainHand => Hand::MainHand,
        InteractionHand::OffHand => Hand::OffHand,
    }
}

pub(crate) const fn interact_action(action: CoreAction) -> InteractAction {
    match action {
        CoreAction::LeftClickBlock => InteractAction::LeftClickBlock,
        CoreAction::RightClickBlock => InteractAction::RightClickBlock,
        CoreAction::RightClickAir => InteractAction::RightClickAir,
        _ => InteractAction::Other,
    }
}

pub(crate) fn entity_info(info: &CoreEntityInfo) -> EntityInfo {
    EntityInfo {
        id: info.id,
        uuid: crate::players::Uuid {
            bytes: *info.uuid.as_bytes(),
        },
        kind: info.kind.to_string().into(),
        player: info
            .is_player
            .then_some(PlayerId {
                bytes: *info.uuid.as_bytes(),
                entity_id: info.id,
            })
            .into(),
        position: position(info.position),
    }
}

pub(crate) fn location(world: &World, pos: DVec3, rotation: (f32, f32)) -> Location {
    Location {
        world: world.key.to_string().into(),
        x: pos.x,
        y: pos.y,
        z: pos.z,
        yaw: rotation.0,
        pitch: rotation.1,
    }
}

pub(crate) fn player_location(player: &Player) -> Location {
    location(&player.get_world(), player.position(), player.rotation())
}

pub(crate) fn redirect_teleport(
    access: &mut TeleportAccess<'_>,
    destination: &Location,
) -> Result<(), String> {
    if !access.writable() {
        return Ok(());
    }
    let key: Identifier = destination.world.as_str().parse().map_err(str::to_owned)?;
    let world = access
        .server()
        .worlds
        .get(&key)
        .ok_or("destination world is not loaded")?;
    if world.domain() != access.event.from_world.domain() {
        return Err("teleport destinations must stay in the player's domain".into());
    }
    let to = DVec3::new(destination.x, destination.y, destination.z);
    if !to.is_finite() || !destination.yaw.is_finite() || !destination.pitch.is_finite() {
        return Err("teleport destination must be finite".into());
    }
    access.event.to_world = world;
    access.event.to = to;
    access.event.rotation = (destination.yaw, destination.pitch);
    Ok(())
}

// Iteration-2 events; the extension bindings call these generated functions directly.
use crate::events::more::api::{
    ChunkLoadEvent, ChunkUnloadEvent, DropItemEvent, EntityDeathEvent, EntitySpawnEvent,
    ExpChangeEvent, ExplosionEvent, FoodLevelChangeEvent, GameModeChangeEvent, InteractEntityEvent,
    InventoryClickEvent, InventoryCloseEvent, InventoryOpenEvent, ItemHeldEvent, PickupItemEvent,
    PistonEvent, PluginMessageEvent, RespawnEvent, ServerCommandEvent, ServerListPingEvent,
    SignChangeEvent, SwapHandsEvent, ToggleEvent, WeatherChangeEvent, WorldChangeEvent,
    WorldLoadEvent, WorldUnloadEvent,
};
use steel_core::extension::events as core;

player_handler!(
    RespawnHandler,
    respawn,
    core::PlayerRespawnEvent,
    RespawnEvent
);
player_handler!(
    WorldChangeHandler,
    world_change,
    core::PlayerChangedWorldEvent,
    WorldChangeEvent
);
player_handler!(
    GameModeHandler,
    game_mode_change,
    core::PlayerGameModeChangeEvent,
    GameModeChangeEvent
);
player_handler!(
    ItemHeldHandler,
    item_held,
    core::PlayerItemHeldEvent,
    ItemHeldEvent
);
player_handler!(ToggleHandler, toggle, core::PlayerToggleEvent, ToggleEvent);
player_handler!(
    SwapHandsHandler,
    swap_hands,
    core::PlayerSwapHandsEvent,
    SwapHandsEvent
);
player_handler!(
    DropItemHandler,
    drop_item,
    core::PlayerDropItemEvent,
    DropItemEvent
);
player_handler!(
    PickupItemHandler,
    pickup_item,
    core::PlayerPickupItemEvent,
    PickupItemEvent
);
player_handler!(
    InteractEntityHandler,
    interact_entity,
    core::PlayerInteractEntityEvent,
    InteractEntityEvent
);
player_handler!(
    InventoryOpenHandler,
    inventory_open,
    core::InventoryOpenEvent,
    InventoryOpenEvent
);
player_handler!(
    InventoryClickHandler,
    inventory_click,
    core::InventoryClickEvent,
    InventoryClickEvent
);
player_handler!(
    InventoryCloseHandler,
    inventory_close,
    core::InventoryCloseEvent,
    InventoryCloseEvent
);
player_handler!(
    SignChangeHandler,
    sign_change,
    core::SignChangeEvent,
    SignChangeEvent
);
player_handler!(
    ExpChangeHandler,
    exp_change,
    core::PlayerExpChangeEvent,
    ExpChangeEvent
);
player_handler!(
    FoodLevelHandler,
    food_level_change,
    core::FoodLevelChangeEvent,
    FoodLevelChangeEvent
);
player_handler!(
    PluginMessageHandler,
    plugin_message,
    core::PluginMessageEvent,
    PluginMessageEvent
);
bare_handler!(
    EntitySpawnHandler,
    entity_spawn,
    core::EntitySpawnEvent,
    EntitySpawnEvent
);
bare_handler!(
    EntityDeathHandler,
    entity_death,
    core::EntityDeathEvent,
    EntityDeathEvent
);
world_handler!(
    ExplosionHandler,
    explosion,
    core::ExplosionEvent,
    ExplosionEvent
);
world_handler!(PistonHandler, piston, core::PistonEvent, PistonEvent);
bare_handler!(
    ChunkLoadHandler,
    chunk_load,
    core::ChunkLoadEvent,
    ChunkLoadEvent
);
bare_handler!(
    ChunkUnloadHandler,
    chunk_unload,
    core::ChunkUnloadEvent,
    ChunkUnloadEvent
);
world_handler!(
    WorldLoadHandler,
    world_load,
    core::WorldLoadEvent,
    WorldLoadEvent
);
bare_handler!(
    WorldUnloadHandler,
    world_unload,
    core::WorldUnloadEvent,
    WorldUnloadEvent
);
bare_handler!(
    WeatherChangeHandler,
    weather_change,
    core::WeatherChangeEvent,
    WeatherChangeEvent
);
bare_handler!(
    ServerCommandHandler,
    server_command,
    core::ServerCommandEvent,
    ServerCommandEvent
);
bare_handler!(
    ServerListPingHandler,
    server_list_ping,
    core::ServerListPingEvent,
    ServerListPingEvent
);

pub(crate) type RespawnAccess<'a> = Access<'a, core::PlayerRespawnEvent>;
pub(crate) type WorldChangeAccess<'a> = Access<'a, core::PlayerChangedWorldEvent>;
pub(crate) type GameModeAccess<'a> = Access<'a, core::PlayerGameModeChangeEvent>;
pub(crate) type ItemHeldAccess<'a> = Access<'a, core::PlayerItemHeldEvent>;
pub(crate) type ToggleAccess<'a> = Access<'a, core::PlayerToggleEvent>;
pub(crate) type SwapHandsAccess<'a> = Access<'a, core::PlayerSwapHandsEvent>;
pub(crate) type DropItemAccess<'a> = Access<'a, core::PlayerDropItemEvent>;
pub(crate) type PickupItemAccess<'a> = Access<'a, core::PlayerPickupItemEvent>;
pub(crate) type InteractEntityAccess<'a> = Access<'a, core::PlayerInteractEntityEvent>;
pub(crate) type InventoryOpenAccess<'a> = Access<'a, core::InventoryOpenEvent>;
pub(crate) type InventoryClickAccess<'a> = Access<'a, core::InventoryClickEvent>;
pub(crate) type InventoryCloseAccess<'a> = Access<'a, core::InventoryCloseEvent>;
pub(crate) type SignChangeAccess<'a> = Access<'a, core::SignChangeEvent>;
pub(crate) type ExpChangeAccess<'a> = Access<'a, core::PlayerExpChangeEvent>;
pub(crate) type FoodLevelAccess<'a> = Access<'a, core::FoodLevelChangeEvent>;
pub(crate) type PluginMessageAccess<'a> = Access<'a, core::PluginMessageEvent>;
pub(crate) type EntitySpawnAccess<'a> = BareAccess<'a, core::EntitySpawnEvent>;
pub(crate) type EntityDeathAccess<'a> = BareAccess<'a, core::EntityDeathEvent>;
pub(crate) type ExplosionAccess<'a> = WorldAccess<'a, core::ExplosionEvent>;
pub(crate) type PistonAccess<'a> = WorldAccess<'a, core::PistonEvent>;
pub(crate) type ChunkLoadAccess<'a> = BareAccess<'a, core::ChunkLoadEvent>;
pub(crate) type ChunkUnloadAccess<'a> = BareAccess<'a, core::ChunkUnloadEvent>;
pub(crate) type WorldLoadAccess<'a> = WorldAccess<'a, core::WorldLoadEvent>;
pub(crate) type WorldUnloadAccess<'a> = BareAccess<'a, core::WorldUnloadEvent>;
pub(crate) type WeatherChangeAccess<'a> = BareAccess<'a, core::WeatherChangeEvent>;
pub(crate) type ServerCommandAccess<'a> = BareAccess<'a, core::ServerCommandEvent>;
pub(crate) type ServerListPingAccess<'a> = BareAccess<'a, core::ServerListPingEvent>;

pub(crate) const fn game_mode(mode: steel_utils::types::GameType) -> crate::GameMode {
    use steel_utils::types::GameType;
    match mode {
        GameType::Survival => crate::GameMode::Survival,
        GameType::Creative => crate::GameMode::Creative,
        GameType::Adventure => crate::GameMode::Adventure,
        GameType::Spectator => crate::GameMode::Spectator,
    }
}

pub(crate) const fn toggle_kind(kind: core::ToggleKind) -> crate::events::ToggleKind {
    match kind {
        core::ToggleKind::Sneak => crate::events::ToggleKind::Sneak,
        core::ToggleKind::Sprint => crate::events::ToggleKind::Sprint,
        core::ToggleKind::Flight => crate::events::ToggleKind::Flight,
        _ => crate::events::ToggleKind::Other,
    }
}

pub(crate) const fn click_kind(
    click: steel_protocol::packets::game::ClickType,
) -> crate::events::ClickKind {
    use steel_protocol::packets::game::ClickType;
    match click {
        ClickType::Pickup => crate::events::ClickKind::Pickup,
        ClickType::QuickMove => crate::events::ClickKind::QuickMove,
        ClickType::Swap => crate::events::ClickKind::Swap,
        ClickType::Clone => crate::events::ClickKind::Clone,
        ClickType::Throw => crate::events::ClickKind::Throw,
        ClickType::QuickCraft => crate::events::ClickKind::QuickCraft,
        ClickType::PickupAll => crate::events::ClickKind::PickupAll,
    }
}

pub(crate) const fn sender_kind(sender: core::CommandSenderKind) -> crate::command::SenderKind {
    match sender {
        core::CommandSenderKind::Console => crate::command::SenderKind::Console,
        _ => crate::command::SenderKind::Other,
    }
}
