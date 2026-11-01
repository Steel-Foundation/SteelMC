//! Cancellable gameplay events fired at vanilla decision points.
//!
//! Vanilla has no event system; this follows the Bukkit/NeoForge model instead. Events are
//! owned structs so handlers never borrow gameplay state, and every bus is fixed before worlds
//! are created. With no subscribers, firing an event does nothing and gameplay is unchanged.
//!
//! Handlers run on the thread processing the action: packet workers for player actions, world
//! tick threads for damage and death. Different players may fire events concurrently.

mod block;
mod entity;
mod inventory;
mod player;
mod server;
mod world;

pub use block::{BlockBreakEvent, BlockPlaceEvent, InteractAction, PlayerInteractEvent};
pub use entity::{EntityDamageEvent, EntityDeathEvent, EntityInfo, EntitySpawnEvent};
pub use inventory::{InventoryClickEvent, InventoryCloseEvent, InventoryOpenEvent};
pub use player::{
    FoodLevelChangeEvent, PlayerChangedWorldEvent, PlayerChatEvent, PlayerCommandEvent,
    PlayerDeathEvent, PlayerDropItemEvent, PlayerExpChangeEvent, PlayerGameModeChangeEvent,
    PlayerInteractEntityEvent, PlayerItemHeldEvent, PlayerJoinEvent, PlayerMoveEvent,
    PlayerPickupItemEvent, PlayerPreLoginEvent, PlayerQuitEvent, PlayerRespawnEvent,
    PlayerSwapHandsEvent, PlayerTeleportEvent, PlayerToggleEvent, SignChangeEvent, TeleportCause,
    ToggleKind,
};
pub use server::{CommandSenderKind, PluginMessageEvent, ServerCommandEvent, ServerListPingEvent};
pub use world::{
    ChunkLoadEvent, ChunkUnloadEvent, ExplosionEvent, PistonEvent, WeatherChangeEvent,
    WorldLoadEvent, WorldUnloadEvent,
};

/// Handler ordering. Monitor handlers observe the final outcome and must not modify it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventPriority {
    /// Runs first; its changes can be overridden by every later handler.
    Lowest,
    /// Runs before normal handlers.
    Low,
    /// Default priority.
    Normal,
    /// Runs after normal handlers.
    High,
    /// Last handler allowed to change the outcome.
    Highest,
    /// Observes the final outcome only.
    Monitor,
}

/// A fired occurrence. Cancellable events override [`Event::is_cancelled`].
pub trait Event: Send {
    /// Whether a handler cancelled the action.
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// One subscriber for an event kind.
pub trait EventHandler<E>: Send + Sync {
    /// Handles one event instance. `priority` is the priority this handler subscribed with.
    fn handle(&self, event: &mut E, priority: EventPriority);
}

struct Subscription<E> {
    priority: EventPriority,
    ignore_cancelled: bool,
    handler: Box<dyn EventHandler<E>>,
}

/// Ordered subscribers for one event kind.
pub struct EventBus<E> {
    subscriptions: Vec<Subscription<E>>,
}

impl<E> Default for EventBus<E> {
    fn default() -> Self {
        Self {
            subscriptions: Vec::new(),
        }
    }
}

impl<E: Event> EventBus<E> {
    /// Adds a handler after every handler with the same or lower priority.
    /// `ignore_cancelled` skips this handler while the event is cancelled.
    pub fn subscribe(
        &mut self,
        priority: EventPriority,
        ignore_cancelled: bool,
        handler: Box<dyn EventHandler<E>>,
    ) {
        let index = self
            .subscriptions
            .partition_point(|subscription| subscription.priority <= priority);
        self.subscriptions.insert(
            index,
            Subscription {
                priority,
                ignore_cancelled,
                handler,
            },
        );
    }

    /// Whether no handler is subscribed. Callers may skip building an event in that case.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.subscriptions.is_empty()
    }

    /// Runs every subscribed handler in priority order.
    pub fn fire(&self, event: &mut E) {
        for subscription in &self.subscriptions {
            if subscription.ignore_cancelled && event.is_cancelled() {
                continue;
            }
            subscription.handler.handle(event, subscription.priority);
        }
    }
}

/// One bus per event kind, installed during startup.
#[derive(Default)]
pub struct Events {
    /// Before a player is admitted; may deny the login.
    pub pre_login: EventBus<PlayerPreLoginEvent>,
    /// After a player joined; may change the join message.
    pub join: EventBus<PlayerJoinEvent>,
    /// After a player left; may change the quit message.
    pub quit: EventBus<PlayerQuitEvent>,
    /// A chat message from a player.
    pub chat: EventBus<PlayerChatEvent>,
    /// A command typed by a player, before parsing.
    pub command: EventBus<PlayerCommandEvent>,
    /// A player block break, before the block is removed.
    pub block_break: EventBus<BlockBreakEvent>,
    /// A player block placement, before the block is set.
    pub block_place: EventBus<BlockPlaceEvent>,
    /// A player click on a block or in the air.
    pub interact: EventBus<PlayerInteractEvent>,
    /// Damage to a living entity, before it is applied.
    pub damage: EventBus<EntityDamageEvent>,
    /// A player death, before the death message is sent.
    pub death: EventBus<PlayerDeathEvent>,
    /// A player moved to a different block position.
    pub movement: EventBus<PlayerMoveEvent>,
    /// A player teleport, before it is applied.
    pub teleport: EventBus<PlayerTeleportEvent>,
    /// A player respawned.
    pub respawn: EventBus<PlayerRespawnEvent>,
    /// A player changed world.
    pub world_change: EventBus<PlayerChangedWorldEvent>,
    /// A player's game mode is about to change.
    pub game_mode_change: EventBus<PlayerGameModeChangeEvent>,
    /// A player selected another hotbar slot.
    pub item_held: EventBus<PlayerItemHeldEvent>,
    /// Sneak, sprint and flight toggles.
    pub toggle: EventBus<PlayerToggleEvent>,
    /// A player swaps hands.
    pub swap_hands: EventBus<PlayerSwapHandsEvent>,
    /// A player drops items.
    pub drop_item: EventBus<PlayerDropItemEvent>,
    /// A player picks up an item entity.
    pub pickup_item: EventBus<PlayerPickupItemEvent>,
    /// A player right-clicks an entity.
    pub interact_entity: EventBus<PlayerInteractEntityEvent>,
    /// A container menu opens.
    pub inventory_open: EventBus<InventoryOpenEvent>,
    /// A container click.
    pub inventory_click: EventBus<InventoryClickEvent>,
    /// A container menu closed.
    pub inventory_close: EventBus<InventoryCloseEvent>,
    /// A sign edit.
    pub sign_change: EventBus<SignChangeEvent>,
    /// A player gains experience.
    pub exp_change: EventBus<PlayerExpChangeEvent>,
    /// A player's food level changed.
    pub food_level_change: EventBus<FoodLevelChangeEvent>,
    /// A non-player entity spawns.
    pub entity_spawn: EventBus<EntitySpawnEvent>,
    /// A non-player living entity died.
    pub entity_death: EventBus<EntityDeathEvent>,
    /// An explosion destroys blocks.
    pub explosion: EventBus<ExplosionEvent>,
    /// A piston moves.
    pub piston: EventBus<PistonEvent>,
    /// A chunk became active.
    pub chunk_load: EventBus<ChunkLoadEvent>,
    /// A chunk unloaded.
    pub chunk_unload: EventBus<ChunkUnloadEvent>,
    /// A world became available.
    pub world_load: EventBus<WorldLoadEvent>,
    /// A world was retired.
    pub world_unload: EventBus<WorldUnloadEvent>,
    /// Rain or thunder changed.
    pub weather_change: EventBus<WeatherChangeEvent>,
    /// A console or remote command.
    pub server_command: EventBus<ServerCommandEvent>,
    /// A server list ping.
    pub server_list_ping: EventBus<ServerListPingEvent>,
    /// A extension-channel message from a player.
    pub plugin_message: EventBus<PluginMessageEvent>,
}

impl Events {
    /// Builds and fires an event only when someone listens; returns the fired event.
    pub fn fire_if<E: Event>(bus: &EventBus<E>, build: impl FnOnce() -> E) -> Option<E> {
        if bus.is_empty() {
            return None;
        }
        let mut event = build();
        bus.fire(&mut event);
        Some(event)
    }
}

#[cfg(test)]
mod tests;
