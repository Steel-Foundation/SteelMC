//! Closure subscriptions built on the native `subscribe_*` bindings.

use super::{
    BlockBreakEvent, BlockPlaceEvent, ChatEvent, ChunkLoadEvent, ChunkUnloadEvent, CommandEvent,
    DamageEvent, DeathEvent, DropItemEvent, EntityDeathEvent, EntitySpawnEvent, EventPriority,
    ExpChangeEvent, ExplosionEvent, FoodLevelChangeEvent, GameModeChangeEvent, InteractEntityEvent,
    InteractEvent, InventoryClickEvent, InventoryCloseEvent, InventoryOpenEvent, ItemHeldEvent,
    JoinEvent, MoveEvent, PickupItemEvent, PistonEvent, PluginMessageEvent, PreLoginEvent,
    QuitEvent, RespawnEvent, ServerCommandEvent, ServerListPingEvent, SignChangeEvent,
    SwapHandsEvent, TeleportEvent, ToggleEvent, WeatherChangeEvent, WorldChangeEvent,
    WorldLoadEvent, WorldUnloadEvent,
};
use crate::{Hooks, Server, callbacks::Table};

/// Defines a closure table, trampoline and `on_*`/`on_*_with` methods for a cancellable event.
macro_rules! cancellable {
    ($table:ident, $trampoline:ident, $event:ident, $subscribe:ident, $on:ident, $on_with:ident, $doc:literal) => {
        static $table: Table<dyn Fn(&mut $event<'_>) + Send + Sync> = Table::new();

        #[stabby::export(callback)]
        fn $trampoline(mut event: $event<'_>, token: u64) {
            if let Some(handler) = $table.get(token) {
                handler(&mut event);
            }
        }

        impl Hooks<'_> {
            #[doc = $doc]
            pub fn $on(
                &mut self,
                priority: EventPriority,
                handler: impl Fn(&mut $event<'_>) + Send + Sync + 'static,
            ) {
                self.$on_with(priority, false, handler);
            }

            #[doc = $doc]
            ///
            /// With `ignore_cancelled`, the handler is skipped while the event is cancelled.
            pub fn $on_with(
                &mut self,
                priority: EventPriority,
                ignore_cancelled: bool,
                handler: impl Fn(&mut $event<'_>) + Send + Sync + 'static,
            ) {
                let token = $table.insert(Box::new(handler));
                self.$subscribe(priority, ignore_cancelled, $trampoline, token);
            }
        }
    };
}

/// Defines a closure table, trampoline and `on_*` method for a non-cancellable event.
macro_rules! observable {
    ($table:ident, $trampoline:ident, $event:ident, $subscribe:ident, $on:ident, $doc:literal) => {
        static $table: Table<dyn Fn(&mut $event<'_>) + Send + Sync> = Table::new();

        #[stabby::export(callback)]
        fn $trampoline(mut event: $event<'_>, token: u64) {
            if let Some(handler) = $table.get(token) {
                handler(&mut event);
            }
        }

        impl Hooks<'_> {
            #[doc = $doc]
            pub fn $on(
                &mut self,
                priority: EventPriority,
                handler: impl Fn(&mut $event<'_>) + Send + Sync + 'static,
            ) {
                let token = $table.insert(Box::new(handler));
                self.$subscribe(priority, $trampoline, token);
            }
        }
    };
}

cancellable!(
    PRE_LOGIN,
    pre_login_trampoline,
    PreLoginEvent,
    subscribe_pre_login,
    on_pre_login,
    on_pre_login_with,
    "Handles a connecting player before admission; deny to reject the login."
);
observable!(
    JOIN,
    join_trampoline,
    JoinEvent,
    subscribe_join,
    on_join,
    "Handles a player that finished joining."
);
observable!(
    QUIT,
    quit_trampoline,
    QuitEvent,
    subscribe_quit,
    on_quit,
    "Handles a player that left."
);
cancellable!(
    CHAT,
    chat_trampoline,
    ChatEvent,
    subscribe_chat,
    on_chat,
    on_chat_with,
    "Handles a chat message."
);
cancellable!(
    COMMAND,
    command_trampoline,
    CommandEvent,
    subscribe_command,
    on_command,
    on_command_with,
    "Handles a typed command before it is parsed."
);
cancellable!(
    BLOCK_BREAK,
    block_break_trampoline,
    BlockBreakEvent,
    subscribe_block_break,
    on_block_break,
    on_block_break_with,
    "Handles a player block break."
);
cancellable!(
    BLOCK_PLACE,
    block_place_trampoline,
    BlockPlaceEvent,
    subscribe_block_place,
    on_block_place,
    on_block_place_with,
    "Handles a player block placement."
);
cancellable!(
    INTERACT,
    interact_trampoline,
    InteractEvent,
    subscribe_interact,
    on_interact,
    on_interact_with,
    "Handles a player click on a block or in the air."
);
cancellable!(
    DAMAGE,
    damage_trampoline,
    DamageEvent,
    subscribe_damage,
    on_damage,
    on_damage_with,
    "Handles damage to a living entity."
);
observable!(
    DEATH,
    death_trampoline,
    DeathEvent,
    subscribe_death,
    on_death,
    "Handles a player death."
);
cancellable!(
    MOVE,
    move_trampoline,
    MoveEvent,
    subscribe_move,
    on_move,
    on_move_with,
    "Handles a player moving into a different block."
);
cancellable!(
    TELEPORT,
    teleport_trampoline,
    TeleportEvent,
    subscribe_teleport,
    on_teleport,
    on_teleport_with,
    "Handles a player teleport."
);

observable!(
    RESPAWN,
    respawn_trampoline,
    RespawnEvent,
    subscribe_respawn,
    on_respawn,
    "Handles a player respawn."
);
observable!(
    WORLD_CHANGE,
    world_change_trampoline,
    WorldChangeEvent,
    subscribe_world_change,
    on_world_change,
    "Handles a player changing world."
);
cancellable!(
    GAME_MODE,
    game_mode_trampoline,
    GameModeChangeEvent,
    subscribe_game_mode_change,
    on_game_mode_change,
    on_game_mode_change_with,
    "Handles a game mode change."
);
cancellable!(
    ITEM_HELD,
    item_held_trampoline,
    ItemHeldEvent,
    subscribe_item_held,
    on_item_held,
    on_item_held_with,
    "Handles a hotbar slot change."
);
cancellable!(
    TOGGLE,
    toggle_trampoline,
    ToggleEvent,
    subscribe_toggle,
    on_toggle,
    on_toggle_with,
    "Handles sneak, sprint and flight toggles."
);
cancellable!(
    SWAP_HANDS,
    swap_hands_trampoline,
    SwapHandsEvent,
    subscribe_swap_hands,
    on_swap_hands,
    on_swap_hands_with,
    "Handles a hand swap."
);
cancellable!(
    DROP_ITEM,
    drop_item_trampoline,
    DropItemEvent,
    subscribe_drop_item,
    on_drop_item,
    on_drop_item_with,
    "Handles a player dropping items."
);
cancellable!(
    PICKUP_ITEM,
    pickup_item_trampoline,
    PickupItemEvent,
    subscribe_pickup_item,
    on_pickup_item,
    on_pickup_item_with,
    "Handles a player picking up an item."
);
cancellable!(
    INTERACT_ENTITY,
    interact_entity_trampoline,
    InteractEntityEvent,
    subscribe_interact_entity,
    on_interact_entity,
    on_interact_entity_with,
    "Handles a right click on an entity."
);
cancellable!(
    INVENTORY_OPEN,
    inventory_open_trampoline,
    InventoryOpenEvent,
    subscribe_inventory_open,
    on_inventory_open,
    on_inventory_open_with,
    "Handles a container menu opening."
);
cancellable!(
    INVENTORY_CLICK,
    inventory_click_trampoline,
    InventoryClickEvent,
    subscribe_inventory_click,
    on_inventory_click,
    on_inventory_click_with,
    "Handles a container click."
);
observable!(
    INVENTORY_CLOSE,
    inventory_close_trampoline,
    InventoryCloseEvent,
    subscribe_inventory_close,
    on_inventory_close,
    "Handles a container menu closing."
);
cancellable!(
    SIGN_CHANGE,
    sign_change_trampoline,
    SignChangeEvent,
    subscribe_sign_change,
    on_sign_change,
    on_sign_change_with,
    "Handles a sign edit."
);
observable!(
    EXP_CHANGE,
    exp_change_trampoline,
    ExpChangeEvent,
    subscribe_exp_change,
    on_exp_change,
    "Handles experience gains."
);
cancellable!(
    FOOD_LEVEL,
    food_level_trampoline,
    FoodLevelChangeEvent,
    subscribe_food_level_change,
    on_food_level_change,
    on_food_level_change_with,
    "Handles food level changes."
);
observable!(
    PLUGIN_MESSAGE,
    plugin_message_trampoline,
    PluginMessageEvent,
    subscribe_plugin_message,
    on_plugin_message,
    "Handles extension-channel messages from players."
);
cancellable!(
    ENTITY_SPAWN,
    entity_spawn_trampoline,
    EntitySpawnEvent,
    subscribe_entity_spawn,
    on_entity_spawn,
    on_entity_spawn_with,
    "Handles a non-player entity spawning."
);
observable!(
    ENTITY_DEATH,
    entity_death_trampoline,
    EntityDeathEvent,
    subscribe_entity_death,
    on_entity_death,
    "Handles a non-player living entity death."
);
cancellable!(
    EXPLOSION,
    explosion_trampoline,
    ExplosionEvent,
    subscribe_explosion,
    on_explosion,
    on_explosion_with,
    "Handles an explosion about to destroy blocks."
);
cancellable!(
    PISTON,
    piston_trampoline,
    PistonEvent,
    subscribe_piston,
    on_piston,
    on_piston_with,
    "Handles a piston about to move."
);
observable!(
    CHUNK_LOAD,
    chunk_load_trampoline,
    ChunkLoadEvent,
    subscribe_chunk_load,
    on_chunk_load,
    "Handles a chunk becoming active."
);
observable!(
    CHUNK_UNLOAD,
    chunk_unload_trampoline,
    ChunkUnloadEvent,
    subscribe_chunk_unload,
    on_chunk_unload,
    "Handles a chunk unloading."
);
observable!(
    WORLD_LOAD,
    world_load_trampoline,
    WorldLoadEvent,
    subscribe_world_load,
    on_world_load,
    "Handles a world becoming available."
);
observable!(
    WORLD_UNLOAD,
    world_unload_trampoline,
    WorldUnloadEvent,
    subscribe_world_unload,
    on_world_unload,
    "Handles a world being retired."
);
cancellable!(
    WEATHER,
    weather_trampoline,
    WeatherChangeEvent,
    subscribe_weather_change,
    on_weather_change,
    on_weather_change_with,
    "Handles a weather change."
);
cancellable!(
    SERVER_COMMAND,
    server_command_trampoline,
    ServerCommandEvent,
    subscribe_server_command,
    on_server_command,
    on_server_command_with,
    "Handles a console command before parsing."
);
observable!(
    SERVER_LIST_PING,
    server_list_ping_trampoline,
    ServerListPingEvent,
    subscribe_server_list_ping,
    on_server_list_ping,
    "Handles a server list ping."
);

static SHUTDOWN: Table<dyn Fn(&Server<'_>) + Send + Sync> = Table::new();

#[stabby::export(callback)]
fn shutdown_trampoline(server: Server<'_>, token: u64) {
    if let Some(handler) = SHUTDOWN.get(token) {
        handler(&server);
    }
}

type TickHandler = dyn Fn(&Server<'_>, u64) + Send + Sync;
static SERVER_TICK: Table<TickHandler> = Table::new();

impl Hooks<'_> {
    /// Runs `handler` once on clean server stop, before worlds are saved.
    pub fn on_shutdown(&mut self, handler: impl Fn(&Server<'_>) + Send + Sync + 'static) {
        let token = SHUTDOWN.insert(Box::new(handler));
        self.register_shutdown(shutdown_trampoline, token);
    }

    /// Runs `handler(server, tick)` at the start of every normal server tick, before world
    /// ticks. Handlers run in subscription order on the server thread.
    pub fn on_server_tick(&mut self, handler: impl Fn(&Server<'_>, u64) + Send + Sync + 'static) {
        SERVER_TICK.insert(Box::new(handler));
        self.enable_scheduler();
    }

    /// Runs [`scheduler`](crate::scheduler) tasks on the server tick. Call during pre-init
    /// before scheduling; tasks scheduled without it never run.
    pub fn enable_scheduler(&mut self) {
        if !SERVER_TICK_INSTALLED.swap(true, core::sync::atomic::Ordering::Relaxed) {
            self.register_server_tick(server_tick_trampoline);
        }
    }
}

static SERVER_TICK_INSTALLED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

#[stabby::export(callback)]
fn server_tick_trampoline(server: Server<'_>, tick: u64) {
    let mut token = 0;
    while let Some(handler) = SERVER_TICK.get(token) {
        handler(&server, tick);
        token += 1;
    }
    crate::scheduler::run_due(&server, tick);
}
