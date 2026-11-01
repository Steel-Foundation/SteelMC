//! Events added in iteration 2: player state, inventories, entities, world and server.
//!
//! Subscribe with the closure helpers on [`Hooks`](crate::Hooks) (`on_respawn`,
//! `on_inventory_click`, ...) or the native `subscribe_*` methods below.

#[stabby::host_api]
/// Iteration-2 event handles and subscriptions.
pub mod api {
    use crate::command::SenderKind;
    use crate::events::{ClickKind, EntityInfo, Face, Hand, Location, ToggleKind};
    use crate::items::ItemSpec;
    use crate::{BlockPos, GameMode, Hooks, PlayerId, Position, Server, World};
    use stabby::{
        option::Option as StableOption, slice::Slice, str::Str, string::String as ApiString,
        vec::Vec as ApiVec,
    };

    #[cfg(feature = "host")]
    use crate::host::events::{
        self as host, ChunkLoadAccess, ChunkUnloadAccess, DropItemAccess, EntityDeathAccess,
        EntitySpawnAccess, ExpChangeAccess, ExplosionAccess, FoodLevelAccess, GameModeAccess,
        InteractEntityAccess, InventoryClickAccess, InventoryCloseAccess, InventoryOpenAccess,
        ItemHeldAccess, PickupItemAccess, PistonAccess, PluginMessageAccess, RespawnAccess,
        ServerCommandAccess, ServerListPingAccess, SignChangeAccess, SwapHandsAccess, ToggleAccess,
        WeatherChangeAccess, WorldChangeAccess, WorldLoadAccess, WorldUnloadAccess,
    };
    #[cfg(feature = "host")]
    use steel_core::entity::Entity;

    /// A player respawned; the player has a new entity ID.
    #[opaque]
    pub struct RespawnEvent<'a>(&'a mut RespawnAccess<'a>);
    /// A player moved to another world.
    #[opaque]
    pub struct WorldChangeEvent<'a>(&'a mut WorldChangeAccess<'a>);
    /// A game mode change.
    #[opaque]
    pub struct GameModeChangeEvent<'a>(&'a mut GameModeAccess<'a>);
    /// A hotbar slot change.
    #[opaque]
    pub struct ItemHeldEvent<'a>(&'a mut ItemHeldAccess<'a>);
    /// A sneak, sprint or flight toggle.
    #[opaque]
    pub struct ToggleEvent<'a>(&'a mut ToggleAccess<'a>);
    /// A main/off hand swap.
    #[opaque]
    pub struct SwapHandsEvent<'a>(&'a mut SwapHandsAccess<'a>);
    /// A player dropping items.
    #[opaque]
    pub struct DropItemEvent<'a>(&'a mut DropItemAccess<'a>);
    /// A player picking up an item entity.
    #[opaque]
    pub struct PickupItemEvent<'a>(&'a mut PickupItemAccess<'a>);
    /// A right click on an entity.
    #[opaque]
    pub struct InteractEntityEvent<'a>(&'a mut InteractEntityAccess<'a>);
    /// A container menu opening.
    #[opaque]
    pub struct InventoryOpenEvent<'a>(&'a mut InventoryOpenAccess<'a>);
    /// A container click.
    #[opaque]
    pub struct InventoryClickEvent<'a>(&'a mut InventoryClickAccess<'a>);
    /// A container menu closed.
    #[opaque]
    pub struct InventoryCloseEvent<'a>(&'a mut InventoryCloseAccess<'a>);
    /// A sign edit.
    #[opaque]
    pub struct SignChangeEvent<'a>(&'a mut SignChangeAccess<'a>);
    /// Gained experience.
    #[opaque]
    pub struct ExpChangeEvent<'a>(&'a mut ExpChangeAccess<'a>);
    /// A food level change.
    #[opaque]
    pub struct FoodLevelChangeEvent<'a>(&'a mut FoodLevelAccess<'a>);
    /// A extension-channel message from a player.
    #[opaque]
    pub struct PluginMessageEvent<'a>(&'a mut PluginMessageAccess<'a>);
    /// A non-player entity spawning.
    #[opaque]
    pub struct EntitySpawnEvent<'a>(&'a mut EntitySpawnAccess<'a>);
    /// A non-player living entity death.
    #[opaque]
    pub struct EntityDeathEvent<'a>(&'a mut EntityDeathAccess<'a>);
    /// An explosion about to destroy blocks.
    #[opaque]
    pub struct ExplosionEvent<'a>(&'a mut ExplosionAccess<'a>);
    /// A piston about to move.
    #[opaque]
    pub struct PistonEvent<'a>(&'a mut PistonAccess<'a>);
    /// A chunk became active.
    #[opaque]
    pub struct ChunkLoadEvent<'a>(&'a mut ChunkLoadAccess<'a>);
    /// A chunk unloaded.
    #[opaque]
    pub struct ChunkUnloadEvent<'a>(&'a mut ChunkUnloadAccess<'a>);
    /// A world became available.
    #[opaque]
    pub struct WorldLoadEvent<'a>(&'a mut WorldLoadAccess<'a>);
    /// A world was retired.
    #[opaque]
    pub struct WorldUnloadEvent<'a>(&'a mut WorldUnloadAccess<'a>);
    /// A weather change.
    #[opaque]
    pub struct WeatherChangeEvent<'a>(&'a mut WeatherChangeAccess<'a>);
    /// A console command.
    #[opaque]
    pub struct ServerCommandEvent<'a>(&'a mut ServerCommandAccess<'a>);
    /// A server list ping.
    #[opaque]
    pub struct ServerListPingEvent<'a>(&'a mut ServerListPingAccess<'a>);

    impl<'a> Hooks<'a> {
        /// Subscribes to respawns.
        pub fn subscribe_respawn(
            &mut self,
            priority: crate::events::EventPriority,
            callback: extern "C" fn(RespawnEvent<'_>, u64),
            token: u64,
        ) {
            host::respawn(self.host_mut(), priority, false, callback, token);
        }
        /// Subscribes to world changes.
        pub fn subscribe_world_change(
            &mut self,
            priority: crate::events::EventPriority,
            callback: extern "C" fn(WorldChangeEvent<'_>, u64),
            token: u64,
        ) {
            host::world_change(self.host_mut(), priority, false, callback, token);
        }
        /// Subscribes to game mode changes.
        pub fn subscribe_game_mode_change(
            &mut self,
            priority: crate::events::EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(GameModeChangeEvent<'_>, u64),
            token: u64,
        ) {
            host::game_mode_change(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes to hotbar slot changes.
        pub fn subscribe_item_held(
            &mut self,
            priority: crate::events::EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(ItemHeldEvent<'_>, u64),
            token: u64,
        ) {
            host::item_held(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes to sneak, sprint and flight toggles.
        pub fn subscribe_toggle(
            &mut self,
            priority: crate::events::EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(ToggleEvent<'_>, u64),
            token: u64,
        ) {
            host::toggle(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes to hand swaps.
        pub fn subscribe_swap_hands(
            &mut self,
            priority: crate::events::EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(SwapHandsEvent<'_>, u64),
            token: u64,
        ) {
            host::swap_hands(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes to item drops.
        pub fn subscribe_drop_item(
            &mut self,
            priority: crate::events::EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(DropItemEvent<'_>, u64),
            token: u64,
        ) {
            host::drop_item(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes to item pickups.
        pub fn subscribe_pickup_item(
            &mut self,
            priority: crate::events::EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(PickupItemEvent<'_>, u64),
            token: u64,
        ) {
            host::pickup_item(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes to entity right clicks.
        pub fn subscribe_interact_entity(
            &mut self,
            priority: crate::events::EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(InteractEntityEvent<'_>, u64),
            token: u64,
        ) {
            host::interact_entity(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes to menu openings.
        pub fn subscribe_inventory_open(
            &mut self,
            priority: crate::events::EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(InventoryOpenEvent<'_>, u64),
            token: u64,
        ) {
            host::inventory_open(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes to container clicks.
        pub fn subscribe_inventory_click(
            &mut self,
            priority: crate::events::EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(InventoryClickEvent<'_>, u64),
            token: u64,
        ) {
            host::inventory_click(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes to menu closes.
        pub fn subscribe_inventory_close(
            &mut self,
            priority: crate::events::EventPriority,
            callback: extern "C" fn(InventoryCloseEvent<'_>, u64),
            token: u64,
        ) {
            host::inventory_close(self.host_mut(), priority, false, callback, token);
        }
        /// Subscribes to sign edits.
        pub fn subscribe_sign_change(
            &mut self,
            priority: crate::events::EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(SignChangeEvent<'_>, u64),
            token: u64,
        ) {
            host::sign_change(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes to experience gains.
        pub fn subscribe_exp_change(
            &mut self,
            priority: crate::events::EventPriority,
            callback: extern "C" fn(ExpChangeEvent<'_>, u64),
            token: u64,
        ) {
            host::exp_change(self.host_mut(), priority, false, callback, token);
        }
        /// Subscribes to food level changes.
        pub fn subscribe_food_level_change(
            &mut self,
            priority: crate::events::EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(FoodLevelChangeEvent<'_>, u64),
            token: u64,
        ) {
            host::food_level_change(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes to extension-channel messages.
        pub fn subscribe_plugin_message(
            &mut self,
            priority: crate::events::EventPriority,
            callback: extern "C" fn(PluginMessageEvent<'_>, u64),
            token: u64,
        ) {
            host::plugin_message(self.host_mut(), priority, false, callback, token);
        }
        /// Subscribes to entity spawns.
        pub fn subscribe_entity_spawn(
            &mut self,
            priority: crate::events::EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(EntitySpawnEvent<'_>, u64),
            token: u64,
        ) {
            host::entity_spawn(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes to living entity deaths.
        pub fn subscribe_entity_death(
            &mut self,
            priority: crate::events::EventPriority,
            callback: extern "C" fn(EntityDeathEvent<'_>, u64),
            token: u64,
        ) {
            host::entity_death(self.host_mut(), priority, false, callback, token);
        }
        /// Subscribes to explosions.
        pub fn subscribe_explosion(
            &mut self,
            priority: crate::events::EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(ExplosionEvent<'_>, u64),
            token: u64,
        ) {
            host::explosion(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes to pistons.
        pub fn subscribe_piston(
            &mut self,
            priority: crate::events::EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(PistonEvent<'_>, u64),
            token: u64,
        ) {
            host::piston(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes to chunk activation.
        pub fn subscribe_chunk_load(
            &mut self,
            priority: crate::events::EventPriority,
            callback: extern "C" fn(ChunkLoadEvent<'_>, u64),
            token: u64,
        ) {
            host::chunk_load(self.host_mut(), priority, false, callback, token);
        }
        /// Subscribes to chunk unloads.
        pub fn subscribe_chunk_unload(
            &mut self,
            priority: crate::events::EventPriority,
            callback: extern "C" fn(ChunkUnloadEvent<'_>, u64),
            token: u64,
        ) {
            host::chunk_unload(self.host_mut(), priority, false, callback, token);
        }
        /// Subscribes to world loads.
        pub fn subscribe_world_load(
            &mut self,
            priority: crate::events::EventPriority,
            callback: extern "C" fn(WorldLoadEvent<'_>, u64),
            token: u64,
        ) {
            host::world_load(self.host_mut(), priority, false, callback, token);
        }
        /// Subscribes to world unloads.
        pub fn subscribe_world_unload(
            &mut self,
            priority: crate::events::EventPriority,
            callback: extern "C" fn(WorldUnloadEvent<'_>, u64),
            token: u64,
        ) {
            host::world_unload(self.host_mut(), priority, false, callback, token);
        }
        /// Subscribes to weather changes.
        pub fn subscribe_weather_change(
            &mut self,
            priority: crate::events::EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(WeatherChangeEvent<'_>, u64),
            token: u64,
        ) {
            host::weather_change(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes to console commands.
        pub fn subscribe_server_command(
            &mut self,
            priority: crate::events::EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(ServerCommandEvent<'_>, u64),
            token: u64,
        ) {
            host::server_command(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes to server list pings.
        pub fn subscribe_server_list_ping(
            &mut self,
            priority: crate::events::EventPriority,
            callback: extern "C" fn(ServerListPingEvent<'_>, u64),
            token: u64,
        ) {
            host::server_list_ping(self.host_mut(), priority, false, callback, token);
        }
    }

    impl<'a> RespawnEvent<'a> {
        /// Respawned player (new entity ID).
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Respawn location.
        #[must_use]
        pub fn location(&self) -> Location {
            let event = &self.host().event;
            host::location(
                &event.player.get_world(),
                event.position,
                event.player.rotation(),
            )
        }
    }

    impl<'a> WorldChangeEvent<'a> {
        /// Moved player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Key of the previous world.
        #[must_use]
        pub fn from(&self) -> ApiString {
            self.host().event.from.to_string().into()
        }
        /// The player's new world.
        #[must_use]
        pub fn world(&self) -> World<'a> {
            World::from_host(self.host().world())
        }
    }

    impl<'a> GameModeChangeEvent<'a> {
        /// Affected player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// New game mode.
        #[must_use]
        pub fn mode(&self) -> GameMode {
            host::game_mode(self.host().event.mode)
        }
        /// Whether the change is refused.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Refuses or allows the change.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> ItemHeldEvent<'a> {
        /// Affected player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Previous hotbar slot.
        #[must_use]
        pub fn previous(&self) -> u8 {
            self.host().event.previous
        }
        /// New hotbar slot.
        #[must_use]
        pub fn new_slot(&self) -> u8 {
            self.host().event.new
        }
        /// Whether the slot change is reverted.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Reverts or allows the slot change.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> ToggleEvent<'a> {
        /// Affected player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Toggled state.
        #[must_use]
        pub fn kind(&self) -> ToggleKind {
            host::toggle_kind(self.host().event.kind)
        }
        /// New value.
        #[must_use]
        pub fn enabled(&self) -> bool {
            self.host().event.enabled
        }
        /// Whether the state stays unchanged.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Keeps or allows the change.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> SwapHandsEvent<'a> {
        /// Affected player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Whether the items stay.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Keeps or allows the swap.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> DropItemEvent<'a> {
        /// Dropping player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// The stack about to be dropped.
        #[must_use]
        pub fn item(&self) -> ItemSpec {
            crate::host::items::to_spec(&self.host().event.item)
        }
        /// Whether the items stay in the inventory.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Keeps or allows the drop.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> PickupItemEvent<'a> {
        /// Picking player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// The stack on the ground.
        #[must_use]
        pub fn item(&self) -> ItemSpec {
            crate::host::items::to_spec(&self.host().event.item)
        }
        /// Item entity ID.
        #[must_use]
        pub fn entity_id(&self) -> i32 {
            self.host().event.entity_id
        }
        /// Whether the item stays on the ground.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Keeps or allows the pickup.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> InteractEntityEvent<'a> {
        /// Interacting player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Clicked entity.
        #[must_use]
        pub fn target(&self) -> EntityInfo {
            host::entity_info(&self.host().event.target)
        }
        /// Hand used.
        #[must_use]
        pub fn hand(&self) -> Hand {
            host::hand(self.host().event.hand)
        }
        /// Whether the interaction is ignored.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Ignores or allows the interaction.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> InventoryOpenEvent<'a> {
        /// Viewing player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Menu title as JSON.
        #[must_use]
        pub fn title(&self) -> ApiString {
            Option::from(host::text_json(Some(&self.host().event.title))).unwrap_or_default()
        }
        /// Whether the menu stays closed.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Keeps closed or allows opening.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> InventoryClickEvent<'a> {
        /// Clicking player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Container ID (0 is the player inventory).
        #[must_use]
        pub fn container_id(&self) -> i32 {
            self.host().event.container_id
        }
        /// Menu ID when this is a extension menu (see [`menus`](crate::menus)).
        #[must_use]
        pub fn menu_id(&self) -> StableOption<u64> {
            let event = &self.host().event;
            crate::host::menus::menu_of(&event.player, event.container_id).into()
        }
        /// Clicked slot, -999 outside the window.
        #[must_use]
        pub fn slot(&self) -> i16 {
            self.host().event.slot
        }
        /// Mouse button or hotbar key.
        #[must_use]
        pub fn button(&self) -> i8 {
            self.host().event.button
        }
        /// Click kind.
        #[must_use]
        pub fn click(&self) -> ClickKind {
            host::click_kind(self.host().event.click)
        }
        /// Whether nothing moves.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Refuses or allows the click.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> InventoryCloseEvent<'a> {
        /// Viewing player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Closed container ID.
        #[must_use]
        pub fn container_id(&self) -> u8 {
            self.host().event.container_id
        }
    }

    impl<'a> SignChangeEvent<'a> {
        /// Editing player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Sign position.
        #[must_use]
        pub fn pos(&self) -> BlockPos {
            host::block_pos(self.host().event.pos)
        }
        /// Whether the front side was edited.
        #[must_use]
        pub fn front(&self) -> bool {
            self.host().event.front
        }
        /// One of the four lines (0–3); empty out of range.
        #[must_use]
        pub fn line(&self, index: u8) -> ApiString {
            self.host()
                .event
                .lines
                .get(usize::from(index))
                .map(|line| line.as_str().into())
                .unwrap_or_default()
        }
        /// Rewrites one line (0–3).
        pub fn set_line(&mut self, index: u8, text: Str<'_>) {
            let access = self.host_mut();
            if access.writable()
                && let Some(line) = access.event.lines.get_mut(usize::from(index))
            {
                text.as_str().clone_into(line);
            }
        }
        /// Whether the sign keeps its old text.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Keeps the old text or allows the edit.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> ExpChangeEvent<'a> {
        /// Affected player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Points to add.
        #[must_use]
        pub fn amount(&self) -> i32 {
            self.host().event.amount
        }
        /// Changes the points to add.
        pub fn set_amount(&mut self, amount: i32) {
            let access = self.host_mut();
            if access.writable() {
                access.event.amount = amount;
            }
        }
    }

    impl<'a> FoodLevelChangeEvent<'a> {
        /// Affected player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Level before the change.
        #[must_use]
        pub fn previous(&self) -> i32 {
            self.host().event.previous
        }
        /// New level.
        #[must_use]
        pub fn level(&self) -> i32 {
            self.host().event.level
        }
        /// Whether the previous level is restored.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Restores the previous level or allows the change.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> PluginMessageEvent<'a> {
        /// Sending player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Channel identifier.
        #[must_use]
        pub fn channel(&self) -> ApiString {
            self.host().event.channel.to_string().into()
        }
        /// Raw payload.
        #[must_use]
        pub fn data(&self) -> ApiVec<u8> {
            self.host().event.data.iter().copied().collect()
        }
    }

    impl<'a> EntitySpawnEvent<'a> {
        /// World key.
        #[must_use]
        pub fn world(&self) -> ApiString {
            self.host().event.world.to_string().into()
        }
        /// Spawning entity.
        #[must_use]
        pub fn entity(&self) -> EntityInfo {
            host::entity_info(&self.host().event.entity)
        }
        /// Whether the entity is not added.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Prevents or allows the spawn.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> EntityDeathEvent<'a> {
        /// World key.
        #[must_use]
        pub fn world(&self) -> ApiString {
            self.host().event.world.to_string().into()
        }
        /// Dead entity.
        #[must_use]
        pub fn entity(&self) -> EntityInfo {
            host::entity_info(&self.host().event.entity)
        }
        /// Whether loot and experience drop.
        #[must_use]
        pub fn drop_loot(&self) -> bool {
            self.host().event.drop_loot
        }
        /// Enables or disables drops.
        pub fn set_drop_loot(&mut self, drop: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.drop_loot = drop;
            }
        }
    }

    impl<'a> ExplosionEvent<'a> {
        /// Exploding world.
        #[must_use]
        pub fn world(&self) -> World<'a> {
            World::from_host(self.host().world())
        }
        /// Explosion center.
        #[must_use]
        pub fn center(&self) -> Position {
            host::position(self.host().event.center)
        }
        /// Radius.
        #[must_use]
        pub fn radius(&self) -> f32 {
            self.host().event.radius
        }
        /// Causing entity, if any.
        #[must_use]
        pub fn source(&self) -> StableOption<EntityInfo> {
            self.host()
                .event
                .source
                .as_ref()
                .map(host::entity_info)
                .into()
        }
        /// Blocks the explosion will destroy.
        #[must_use]
        pub fn blocks(&self) -> ApiVec<BlockPos> {
            self.host()
                .event
                .blocks
                .iter()
                .map(|pos| host::block_pos(*pos))
                .collect()
        }
        /// Replaces the destroyed block list (use a subset to spare blocks).
        pub fn set_blocks(&mut self, blocks: Slice<'_, BlockPos>) {
            let access = self.host_mut();
            if access.writable() {
                access.event.blocks = blocks
                    .iter()
                    .map(|pos| steel_utils::BlockPos::new(pos.x, pos.y, pos.z))
                    .collect();
            }
        }
        /// Whether no block is destroyed.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Spares every block, or not.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> PistonEvent<'a> {
        /// Piston world.
        #[must_use]
        pub fn world(&self) -> World<'a> {
            World::from_host(self.host().world())
        }
        /// Piston position.
        #[must_use]
        pub fn pos(&self) -> BlockPos {
            host::block_pos(self.host().event.pos)
        }
        /// Facing direction.
        #[must_use]
        pub fn direction(&self) -> Face {
            host::face(self.host().event.direction)
        }
        /// Whether it extends.
        #[must_use]
        pub fn extending(&self) -> bool {
            self.host().event.extending
        }
        /// Blocks it will move or destroy.
        #[must_use]
        pub fn blocks(&self) -> ApiVec<BlockPos> {
            self.host()
                .event
                .blocks
                .iter()
                .map(|pos| host::block_pos(*pos))
                .collect()
        }
        /// Whether the piston stays.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Stops or allows the move.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> ChunkLoadEvent<'a> {
        /// World key.
        #[must_use]
        pub fn world(&self) -> ApiString {
            self.host().event.world.to_string().into()
        }
        /// Chunk X.
        #[must_use]
        pub fn x(&self) -> i32 {
            self.host().event.pos.0.x
        }
        /// Chunk Z.
        #[must_use]
        pub fn z(&self) -> i32 {
            self.host().event.pos.0.y
        }
    }

    impl<'a> ChunkUnloadEvent<'a> {
        /// World key.
        #[must_use]
        pub fn world(&self) -> ApiString {
            self.host().event.world.to_string().into()
        }
        /// Chunk X.
        #[must_use]
        pub fn x(&self) -> i32 {
            self.host().event.pos.0.x
        }
        /// Chunk Z.
        #[must_use]
        pub fn z(&self) -> i32 {
            self.host().event.pos.0.y
        }
    }

    impl<'a> WorldLoadEvent<'a> {
        /// Loaded world.
        #[must_use]
        pub fn world(&self) -> World<'a> {
            World::from_host(self.host().world())
        }
    }

    impl<'a> WorldUnloadEvent<'a> {
        /// World key.
        #[must_use]
        pub fn world(&self) -> ApiString {
            self.host().event.world.to_string().into()
        }
    }

    impl<'a> WeatherChangeEvent<'a> {
        /// World key.
        #[must_use]
        pub fn world(&self) -> ApiString {
            self.host().event.world.to_string().into()
        }
        /// New rain state.
        #[must_use]
        pub fn raining(&self) -> bool {
            self.host().event.raining
        }
        /// New thunder state.
        #[must_use]
        pub fn thundering(&self) -> bool {
            self.host().event.thundering
        }
        /// Whether the previous weather is restored.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Restores or allows the change.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> ServerCommandEvent<'a> {
        /// Sender kind (`Console` or `Other` for remote consoles).
        #[must_use]
        pub fn sender(&self) -> SenderKind {
            host::sender_kind(self.host().event.sender)
        }
        /// Command line without a leading `/`.
        #[must_use]
        pub fn command(&self) -> ApiString {
            self.host().event.command.as_str().into()
        }
        /// Replaces the command line.
        pub fn set_command(&mut self, command: Str<'_>) {
            let access = self.host_mut();
            if access.writable() {
                command.as_str().clone_into(&mut access.event.command);
            }
        }
        /// Whether the command is dropped.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Drops or allows the command.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> ServerListPingEvent<'a> {
        /// Message of the day (plain text, `§` codes allowed).
        #[must_use]
        pub fn motd(&self) -> ApiString {
            self.host().event.motd.as_str().into()
        }
        /// Replaces the message of the day.
        pub fn set_motd(&mut self, motd: Str<'_>) {
            let access = self.host_mut();
            if access.writable() {
                motd.as_str().clone_into(&mut access.event.motd);
            }
        }
        /// Displayed player limit.
        #[must_use]
        pub fn max_players(&self) -> i32 {
            self.host().event.max_players
        }
        /// Changes the displayed player limit.
        pub fn set_max_players(&mut self, max: i32) {
            let access = self.host_mut();
            if access.writable() {
                access.event.max_players = max;
            }
        }
        /// Displayed online count.
        #[must_use]
        pub fn online_players(&self) -> i32 {
            self.host().event.online_players
        }
        /// Changes the displayed online count.
        pub fn set_online_players(&mut self, online: i32) {
            let access = self.host_mut();
            if access.writable() {
                access.event.online_players = online;
            }
        }
    }
}
