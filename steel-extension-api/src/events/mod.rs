//! Cancellable gameplay events.
//!
//! Subscribe during pre-init with the closure helpers on [`Hooks`](crate::Hooks), for example
//! `hooks.on_block_break(EventPriority::Normal, |event| event.set_cancelled(true))`.
//!
//! Handlers run synchronously on the thread processing the action: a packet worker for player
//! actions, a world thread for damage and death. Handlers for different players can run at the
//! same time, so extension state must be thread-safe. Event handles borrow host state and cannot
//! leave the handler:
//!
//! ```compile_fail
//! fn retain(event: steel_extension_api::events::ChatEvent<'_>) -> steel_extension_api::events::ChatEvent<'static> {
//!     event
//! }
//! ```
//!
//! Handlers at [`EventPriority::Monitor`] observe the final outcome; their setters do nothing.

mod handlers;
pub mod more;

use crate::{BlockPos, PlayerId, Position};
use stabby::string::String as ApiString;

pub use api::{
    BlockBreakEvent, BlockPlaceEvent, ChatEvent, CommandEvent, DamageEvent, DeathEvent,
    InteractEvent, JoinEvent, MoveEvent, PreLoginEvent, QuitEvent, TeleportEvent,
};
pub use more::api::{
    ChunkLoadEvent, ChunkUnloadEvent, DropItemEvent, EntityDeathEvent, EntitySpawnEvent,
    ExpChangeEvent, ExplosionEvent, FoodLevelChangeEvent, GameModeChangeEvent, InteractEntityEvent,
    InventoryClickEvent, InventoryCloseEvent, InventoryOpenEvent, ItemHeldEvent, PickupItemEvent,
    PistonEvent, PluginMessageEvent, RespawnEvent, ServerCommandEvent, ServerListPingEvent,
    SignChangeEvent, SwapHandsEvent, ToggleEvent, WeatherChangeEvent, WorldChangeEvent,
    WorldLoadEvent, WorldUnloadEvent,
};

/// Which player state a [`ToggleEvent`] changes.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleKind {
    /// Sneaking.
    Sneak,
    /// Sprinting.
    Sprint,
    /// Creative-style flight.
    Flight,
    /// A state added by a newer host.
    Other,
}

/// Vanilla container click kinds.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickKind {
    /// Normal left/right click.
    Pickup,
    /// Shift-click.
    QuickMove,
    /// Number key or off-hand swap (`button` holds the hotbar slot, 40 for off hand).
    Swap,
    /// Middle click in creative.
    Clone,
    /// Drop key (Q).
    Throw,
    /// Drag across slots.
    QuickCraft,
    /// Double click to collect.
    PickupAll,
}

/// Handler ordering; see the module documentation.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventPriority {
    /// Runs first.
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

/// The click that caused an [`InteractEvent`].
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractAction {
    /// Started breaking (left click) a block.
    LeftClickBlock,
    /// Used an item on (right click) a block.
    RightClickBlock,
    /// Used an item without targeting a block.
    RightClickAir,
    /// A click kind added by a newer host; treat it as unknown.
    Other,
}

/// The hand used for an interaction.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hand {
    /// Main hand.
    MainHand,
    /// Off hand.
    OffHand,
}

/// A block face.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// Negative Y.
    Down,
    /// Positive Y.
    Up,
    /// Negative Z.
    North,
    /// Positive Z.
    South,
    /// Negative X.
    West,
    /// Positive X.
    East,
}

impl Face {
    /// The block adjacent to `pos` on this face.
    #[must_use]
    pub const fn relative(self, pos: BlockPos) -> BlockPos {
        let (x, y, z) = match self {
            Self::Down => (0, -1, 0),
            Self::Up => (0, 1, 0),
            Self::North => (0, 0, -1),
            Self::South => (0, 0, 1),
            Self::West => (-1, 0, 0),
            Self::East => (1, 0, 0),
        };
        BlockPos {
            x: pos.x + x,
            y: pos.y + y,
            z: pos.z + z,
        }
    }
}

/// A world position with rotation.
#[stabby::stabby]
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    /// World key (`domain:world`).
    pub world: ApiString,
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
    /// Z coordinate.
    pub z: f64,
    /// Horizontal rotation in degrees.
    pub yaw: f32,
    /// Vertical rotation in degrees.
    pub pitch: f32,
}

impl Location {
    /// The position without rotation.
    #[must_use]
    pub const fn position(&self) -> Position {
        Position {
            x: self.x,
            y: self.y,
            z: self.z,
        }
    }

    /// The containing block.
    #[must_use]
    pub const fn block(&self) -> BlockPos {
        self.position().block()
    }
}

impl Position {
    /// The containing block.
    #[must_use]
    pub const fn block(self) -> BlockPos {
        // Truncation after floor is the vanilla block position rule.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "world coordinates are within i32 range"
        )]
        BlockPos {
            x: self.x.floor() as i32,
            y: self.y.floor() as i32,
            z: self.z.floor() as i32,
        }
    }
}

/// Identity of an entity involved in an event.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct EntityInfo {
    /// Network entity ID.
    pub id: i32,
    /// Persistent UUID.
    pub uuid: crate::players::Uuid,
    /// Entity type key, such as `minecraft:zombie`.
    pub kind: ApiString,
    /// The player identity when the entity is a player.
    pub player: stabby::option::Option<PlayerId>,
    /// Position when the event fired.
    pub position: Position,
}

#[stabby::host_api]
/// Event handles and subscriptions.
pub mod api {
    use super::{EntityInfo, EventPriority, Face, Hand, InteractAction, Location};
    use crate::{BlockPos, BlockState, Hooks, PlayerId, Position, Server, World, players::Uuid};
    use stabby::{option::Option as StableOption, str::Str, string::String as ApiString};

    use crate::ApiResult;
    #[cfg(feature = "host")]
    use crate::host::events::{
        self as host, BreakAccess, ChatAccess, CommandAccess, DamageAccess, DeathAccess,
        InteractAccess, JoinAccess, MoveAccess, PlaceAccess, PreLoginAccess, QuitAccess,
        TeleportAccess,
    };
    #[cfg(feature = "host")]
    use steel_core::entity::Entity;

    /// A connecting player, before admission.
    #[opaque]
    pub struct PreLoginEvent<'a>(&'a mut PreLoginAccess<'a>);
    /// A player finished joining.
    #[opaque]
    pub struct JoinEvent<'a>(&'a mut JoinAccess<'a>);
    /// A player left.
    #[opaque]
    pub struct QuitEvent<'a>(&'a mut QuitAccess<'a>);
    /// A chat message.
    #[opaque]
    pub struct ChatEvent<'a>(&'a mut ChatAccess<'a>);
    /// A command typed by a player, before parsing.
    #[opaque]
    pub struct CommandEvent<'a>(&'a mut CommandAccess<'a>);
    /// A player block break.
    #[opaque]
    pub struct BlockBreakEvent<'a>(&'a mut BreakAccess<'a>);
    /// A player block placement.
    #[opaque]
    pub struct BlockPlaceEvent<'a>(&'a mut PlaceAccess<'a>);
    /// A player click.
    #[opaque]
    pub struct InteractEvent<'a>(&'a mut InteractAccess<'a>);
    /// Damage to a living entity.
    #[opaque]
    pub struct DamageEvent<'a>(&'a mut DamageAccess<'a>);
    /// A player death.
    #[opaque]
    pub struct DeathEvent<'a>(&'a mut DeathAccess<'a>);
    /// A player moved into a different block.
    #[opaque]
    pub struct MoveEvent<'a>(&'a mut MoveAccess<'a>);
    /// A player teleport.
    #[opaque]
    pub struct TeleportEvent<'a>(&'a mut TeleportAccess<'a>);

    impl<'a> Hooks<'a> {
        /// Subscribes `callback(event, token)` to pre-login.
        pub fn subscribe_pre_login(
            &mut self,
            priority: EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(PreLoginEvent<'_>, u64),
            token: u64,
        ) {
            host::subscribe_pre_login(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes `callback(event, token)` to join.
        pub fn subscribe_join(
            &mut self,
            priority: EventPriority,
            callback: extern "C" fn(JoinEvent<'_>, u64),
            token: u64,
        ) {
            host::subscribe_join(self.host_mut(), priority, callback, token);
        }
        /// Subscribes `callback(event, token)` to quit.
        pub fn subscribe_quit(
            &mut self,
            priority: EventPriority,
            callback: extern "C" fn(QuitEvent<'_>, u64),
            token: u64,
        ) {
            host::subscribe_quit(self.host_mut(), priority, callback, token);
        }
        /// Subscribes `callback(event, token)` to chat.
        pub fn subscribe_chat(
            &mut self,
            priority: EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(ChatEvent<'_>, u64),
            token: u64,
        ) {
            host::subscribe_chat(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes `callback(event, token)` to command preprocess.
        pub fn subscribe_command(
            &mut self,
            priority: EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(CommandEvent<'_>, u64),
            token: u64,
        ) {
            host::subscribe_command(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes `callback(event, token)` to block breaks.
        pub fn subscribe_block_break(
            &mut self,
            priority: EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(BlockBreakEvent<'_>, u64),
            token: u64,
        ) {
            host::subscribe_block_break(
                self.host_mut(),
                priority,
                ignore_cancelled,
                callback,
                token,
            );
        }
        /// Subscribes `callback(event, token)` to block placements.
        pub fn subscribe_block_place(
            &mut self,
            priority: EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(BlockPlaceEvent<'_>, u64),
            token: u64,
        ) {
            host::subscribe_block_place(
                self.host_mut(),
                priority,
                ignore_cancelled,
                callback,
                token,
            );
        }
        /// Subscribes `callback(event, token)` to clicks.
        pub fn subscribe_interact(
            &mut self,
            priority: EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(InteractEvent<'_>, u64),
            token: u64,
        ) {
            host::subscribe_interact(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes `callback(event, token)` to damage.
        pub fn subscribe_damage(
            &mut self,
            priority: EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(DamageEvent<'_>, u64),
            token: u64,
        ) {
            host::subscribe_damage(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes `callback(event, token)` to player deaths.
        pub fn subscribe_death(
            &mut self,
            priority: EventPriority,
            callback: extern "C" fn(DeathEvent<'_>, u64),
            token: u64,
        ) {
            host::subscribe_death(self.host_mut(), priority, callback, token);
        }
        /// Subscribes `callback(event, token)` to block-changing movement.
        pub fn subscribe_move(
            &mut self,
            priority: EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(MoveEvent<'_>, u64),
            token: u64,
        ) {
            host::subscribe_move(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
        /// Subscribes `callback(event, token)` to teleports.
        pub fn subscribe_teleport(
            &mut self,
            priority: EventPriority,
            ignore_cancelled: bool,
            callback: extern "C" fn(TeleportEvent<'_>, u64),
            token: u64,
        ) {
            host::subscribe_teleport(self.host_mut(), priority, ignore_cancelled, callback, token);
        }
    }

    impl<'a> PreLoginEvent<'a> {
        /// Profile UUID.
        #[must_use]
        pub fn uuid(&self) -> Uuid {
            Uuid {
                bytes: *self.host().event.uuid.as_bytes(),
            }
        }
        /// Profile name.
        #[must_use]
        pub fn name(&self) -> ApiString {
            self.host().event.name.as_str().into()
        }
        /// Whether a handler denied the login.
        #[must_use]
        pub fn is_denied(&self) -> bool {
            self.host().event.denied.is_some()
        }
        /// Denies the login with a text (JSON or plain) reason.
        pub fn deny(&mut self, reason: Str<'_>) {
            let access = self.host_mut();
            if access.writable() {
                access.event.denied = Some(host::parse_text(reason.as_str()));
            }
        }
        /// Allows the login again.
        pub fn allow(&mut self) {
            let access = self.host_mut();
            if access.writable() {
                access.event.denied = None;
            }
        }
    }

    impl<'a> JoinEvent<'a> {
        /// Joined player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server owning the player.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Broadcast message as JSON, if any.
        #[must_use]
        pub fn message(&self) -> StableOption<ApiString> {
            host::text_json(self.host().event.message.as_ref())
        }
        /// Replaces the broadcast message (JSON or plain text).
        pub fn set_message(&mut self, message: Str<'_>) {
            let access = self.host_mut();
            if access.writable() {
                access.event.message = Some(host::parse_text(message.as_str()));
            }
        }
        /// Suppresses the broadcast.
        pub fn clear_message(&mut self) {
            let access = self.host_mut();
            if access.writable() {
                access.event.message = None;
            }
        }
    }

    impl<'a> QuitEvent<'a> {
        /// Leaving player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server owning the player.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Broadcast message as JSON, if any.
        #[must_use]
        pub fn message(&self) -> StableOption<ApiString> {
            host::text_json(self.host().event.message.as_ref())
        }
        /// Replaces the broadcast message (JSON or plain text).
        pub fn set_message(&mut self, message: Str<'_>) {
            let access = self.host_mut();
            if access.writable() {
                access.event.message = Some(host::parse_text(message.as_str()));
            }
        }
        /// Suppresses the broadcast.
        pub fn clear_message(&mut self) {
            let access = self.host_mut();
            if access.writable() {
                access.event.message = None;
            }
        }
    }

    impl<'a> ChatEvent<'a> {
        /// Sender.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server owning the player.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Message text.
        #[must_use]
        pub fn message(&self) -> ApiString {
            self.host().event.message.as_str().into()
        }
        /// Replaces the message text; edited messages are relayed unsigned.
        pub fn set_message(&mut self, message: Str<'_>) {
            let access = self.host_mut();
            if access.writable() {
                message.as_str().clone_into(&mut access.event.message);
            }
        }
        /// Delivers this complete component (JSON or plain) to every player instead of the
        /// vanilla-decorated message. Steel then relays it as system chat.
        pub fn set_rendered(&mut self, rendered: Str<'_>) {
            let access = self.host_mut();
            if access.writable() {
                access.event.rendered = Some(host::parse_text(rendered.as_str()));
            }
        }
        /// Whether the message is dropped.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Drops or restores the message.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> CommandEvent<'a> {
        /// Sender.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server owning the player.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Command line without the leading `/`.
        #[must_use]
        pub fn command(&self) -> ApiString {
            self.host().event.command.as_str().into()
        }
        /// Replaces the command line to dispatch.
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
        /// Drops or restores the command.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> BlockBreakEvent<'a> {
        /// Breaking player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server owning the player.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Player's world.
        #[must_use]
        pub fn world(&self) -> World<'a> {
            World::from_host(self.host().world())
        }
        /// Broken block.
        #[must_use]
        pub fn pos(&self) -> BlockPos {
            host::block_pos(self.host().event.pos)
        }
        /// State before breaking.
        #[must_use]
        pub fn state(&self) -> BlockState {
            BlockState {
                id: self.host().event.state.0,
            }
        }
        /// Whether loot and experience drop.
        #[must_use]
        pub fn drop_items(&self) -> bool {
            self.host().event.drop_items
        }
        /// Enables or disables loot (the block still breaks).
        pub fn set_drop_items(&mut self, drop: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.drop_items = drop;
            }
        }
        /// Whether the block stays.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Keeps or releases the block.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> BlockPlaceEvent<'a> {
        /// Placing player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server owning the player.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Player's world.
        #[must_use]
        pub fn world(&self) -> World<'a> {
            World::from_host(self.host().world())
        }
        /// Placement position.
        #[must_use]
        pub fn pos(&self) -> BlockPos {
            host::block_pos(self.host().event.pos)
        }
        /// Clicked block the placement is against.
        #[must_use]
        pub fn against(&self) -> BlockPos {
            host::block_pos(self.host().event.against)
        }
        /// State being placed.
        #[must_use]
        pub fn state(&self) -> BlockState {
            BlockState {
                id: self.host().event.state.0,
            }
        }
        /// Whether the placement fails.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Fails or restores the placement.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> InteractEvent<'a> {
        /// Clicking player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server owning the player.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Player's world.
        #[must_use]
        pub fn world(&self) -> World<'a> {
            World::from_host(self.host().world())
        }
        /// Click kind.
        #[must_use]
        pub fn action(&self) -> InteractAction {
            host::interact_action(self.host().event.action)
        }
        /// Hand used.
        #[must_use]
        pub fn hand(&self) -> Hand {
            host::hand(self.host().event.hand)
        }
        /// Clicked block, if any.
        #[must_use]
        pub fn pos(&self) -> StableOption<BlockPos> {
            self.host().event.pos.map(host::block_pos).into()
        }
        /// Clicked face, if any.
        #[must_use]
        pub fn face(&self) -> StableOption<Face> {
            self.host().event.face.map(host::face).into()
        }
        /// Held item key (`minecraft:air` when empty).
        #[must_use]
        pub fn item(&self) -> ApiString {
            self.host().event.item.to_string().into()
        }
        /// Whether the click is ignored.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Ignores or restores the click.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> DamageEvent<'a> {
        /// Key of the world the damage happens in.
        #[must_use]
        pub fn world(&self) -> ApiString {
            self.host().event.world.to_string().into()
        }
        /// Damaged entity.
        #[must_use]
        pub fn victim(&self) -> EntityInfo {
            host::entity_info(&self.host().event.victim)
        }
        /// Entity responsible for the damage, if any.
        #[must_use]
        pub fn attacker(&self) -> StableOption<EntityInfo> {
            self.host()
                .event
                .attacker
                .as_ref()
                .map(host::entity_info)
                .into()
        }
        /// Damage type key, such as `minecraft:fall`.
        #[must_use]
        pub fn cause(&self) -> ApiString {
            self.host().event.cause.to_string().into()
        }
        /// Damage amount before armor.
        #[must_use]
        pub fn amount(&self) -> f32 {
            self.host().event.amount
        }
        /// Changes the damage amount; negative and non-finite values are ignored.
        pub fn set_amount(&mut self, amount: f32) {
            let access = self.host_mut();
            if access.writable() && amount.is_finite() && amount >= 0.0 {
                access.event.amount = amount;
            }
        }
        /// Whether no damage is dealt.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Prevents or restores the damage.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> DeathEvent<'a> {
        /// Dead player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server owning the player.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Death location.
        #[must_use]
        pub fn location(&self) -> Location {
            host::player_location(&self.host().event.player)
        }
        /// Death message as JSON, if any.
        #[must_use]
        pub fn message(&self) -> StableOption<ApiString> {
            host::text_json(self.host().event.message.as_ref())
        }
        /// Replaces the death message (JSON or plain text).
        pub fn set_message(&mut self, message: Str<'_>) {
            let access = self.host_mut();
            if access.writable() {
                access.event.message = Some(host::parse_text(message.as_str()));
            }
        }
        /// Suppresses the death message.
        pub fn clear_message(&mut self) {
            let access = self.host_mut();
            if access.writable() {
                access.event.message = None;
            }
        }
    }

    impl<'a> MoveEvent<'a> {
        /// Moving player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server owning the player.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Key of the player's world.
        #[must_use]
        pub fn world(&self) -> ApiString {
            self.host().event.player.get_world().key.to_string().into()
        }
        /// Previous position.
        #[must_use]
        pub fn from(&self) -> Position {
            host::position(self.host().event.from)
        }
        /// New position.
        #[must_use]
        pub fn to(&self) -> Position {
            host::position(self.host().event.to)
        }
        /// Whether the movement is reverted.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Reverts or restores the movement.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }

    impl<'a> TeleportEvent<'a> {
        /// Teleported player.
        #[must_use]
        pub fn player(&self) -> PlayerId {
            host::player_id(&self.host().event.player)
        }
        /// Server owning the player.
        #[must_use]
        pub fn server(&self) -> Server<'a> {
            Server::from_host(self.host().server())
        }
        /// Current location.
        #[must_use]
        pub fn from(&self) -> Location {
            let event = &self.host().event;
            host::location(&event.from_world, event.from, event.player.rotation())
        }
        /// Destination.
        #[must_use]
        pub fn to(&self) -> Location {
            let event = &self.host().event;
            host::location(&event.to_world, event.to, event.rotation)
        }
        /// Teleport cause key, such as `steel:command`.
        #[must_use]
        pub fn cause(&self) -> ApiString {
            self.host().event.cause.to_string().into()
        }
        /// Redirects the teleport to a loaded world in the player's domain.
        pub fn set_to(&mut self, destination: Location) -> ApiResult<()> {
            host::redirect_teleport(self.host_mut(), &destination)
                .map_err(ApiString::from)
                .into()
        }
        /// Whether the teleport is dropped.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.host().event.cancelled
        }
        /// Drops or restores the teleport.
        pub fn set_cancelled(&mut self, cancelled: bool) {
            let access = self.host_mut();
            if access.writable() {
                access.event.cancelled = cancelled;
            }
        }
    }
}
