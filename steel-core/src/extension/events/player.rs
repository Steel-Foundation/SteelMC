//! Player lifecycle, chat and movement events.

use std::sync::Arc;

use glam::DVec3;
use steel_utils::Identifier;
use text_components::TextComponent;
use uuid::Uuid;

use super::Event;
use crate::{player::Player, world::World};

/// A connecting player before admission. Setting `denied` rejects the login with that reason.
pub struct PlayerPreLoginEvent {
    /// Profile UUID.
    pub uuid: Uuid,
    /// Profile name.
    pub name: String,
    /// Disconnect reason when denied.
    pub denied: Option<TextComponent>,
}

impl Event for PlayerPreLoginEvent {
    fn is_cancelled(&self) -> bool {
        self.denied.is_some()
    }
}

/// A player finished joining. `None` suppresses the join broadcast.
pub struct PlayerJoinEvent {
    /// Joined player.
    pub player: Arc<Player>,
    /// Broadcast message.
    pub message: Option<TextComponent>,
}

impl Event for PlayerJoinEvent {}

/// A player left. `None` suppresses the leave broadcast.
pub struct PlayerQuitEvent {
    /// Leaving player.
    pub player: Arc<Player>,
    /// Broadcast message.
    pub message: Option<TextComponent>,
}

impl Event for PlayerQuitEvent {}

/// A chat message.
///
/// When `rendered` is set, recipients receive it as system chat instead of the signed player
/// message, like Paper's chat renderer. Otherwise the (possibly edited) message is relayed with
/// vanilla formatting; editing the text drops its signature.
pub struct PlayerChatEvent {
    /// Sender.
    pub player: Arc<Player>,
    /// Message text.
    pub message: String,
    /// Complete component shown to recipients, replacing vanilla decoration.
    pub rendered: Option<TextComponent>,
    /// Whether the message is dropped.
    pub cancelled: bool,
}

impl Event for PlayerChatEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A command line typed by a player, without the leading `/`.
pub struct PlayerCommandEvent {
    /// Sender.
    pub player: Arc<Player>,
    /// Command line to dispatch.
    pub command: String,
    /// Whether the command is dropped.
    pub cancelled: bool,
}

impl Event for PlayerCommandEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A player moved into a different block. Cancelling returns them to `from`.
pub struct PlayerMoveEvent {
    /// Moving player.
    pub player: Arc<Player>,
    /// Previous accepted position.
    pub from: DVec3,
    /// New position.
    pub to: DVec3,
    /// Whether the movement is reverted.
    pub cancelled: bool,
}

impl Event for PlayerMoveEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// Why a player teleport happened. Identifier keyed so mods can add their own causes.
pub struct TeleportCause;

impl TeleportCause {
    /// `/tp` and other commands.
    pub const COMMAND: Identifier = Identifier::new_static("steel", "command");
    /// A extension request.
    pub const PLUGIN: Identifier = Identifier::new_static("steel", "extension");
    /// Any other server-side teleport.
    pub const UNKNOWN: Identifier = Identifier::new_static("steel", "unknown");
}

/// A player teleport before it is applied. Handlers may redirect it within the same domain.
pub struct PlayerTeleportEvent {
    /// Teleported player.
    pub player: Arc<Player>,
    /// Current world.
    pub from_world: Arc<World>,
    /// Current position.
    pub from: DVec3,
    /// Destination world.
    pub to_world: Arc<World>,
    /// Destination position.
    pub to: DVec3,
    /// Destination yaw and pitch.
    pub rotation: (f32, f32),
    /// Teleport cause.
    pub cause: Identifier,
    /// Whether the teleport is dropped.
    pub cancelled: bool,
}

impl Event for PlayerTeleportEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A player died. `None` suppresses the death message.
pub struct PlayerDeathEvent {
    /// Dead player.
    pub player: Arc<Player>,
    /// Death message.
    pub message: Option<TextComponent>,
}

impl Event for PlayerDeathEvent {}

/// A player respawned after death or the end credits. The player is the respawned replacement,
/// which has a new entity ID.
pub struct PlayerRespawnEvent {
    /// Respawned player.
    pub player: Arc<Player>,
    /// Respawn position.
    pub position: DVec3,
}

impl Event for PlayerRespawnEvent {}

/// A player finished moving to another world of the same domain.
pub struct PlayerChangedWorldEvent {
    /// Moved player.
    pub player: Arc<Player>,
    /// Key of the previous world.
    pub from: Identifier,
}

impl Event for PlayerChangedWorldEvent {}

/// A player's game mode is about to change.
pub struct PlayerGameModeChangeEvent {
    /// Affected player.
    pub player: Arc<Player>,
    /// New game mode.
    pub mode: steel_utils::types::GameType,
    /// Whether the change is refused.
    pub cancelled: bool,
}

impl Event for PlayerGameModeChangeEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A player selected another hotbar slot.
pub struct PlayerItemHeldEvent {
    /// Affected player.
    pub player: Arc<Player>,
    /// Previously selected slot (0–8).
    pub previous: u8,
    /// Newly selected slot (0–8).
    pub new: u8,
    /// Whether the client is returned to the previous slot.
    pub cancelled: bool,
}

impl Event for PlayerItemHeldEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// Which player state a [`PlayerToggleEvent`] changes. Non-exhaustive so mods can add states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToggleKind {
    /// Sneaking (crouching).
    Sneak,
    /// Sprinting.
    Sprint,
    /// Creative-style flight.
    Flight,
}

/// A player starts or stops sneaking, sprinting or flying.
pub struct PlayerToggleEvent {
    /// Affected player.
    pub player: Arc<Player>,
    /// Toggled state.
    pub kind: ToggleKind,
    /// New value of the state.
    pub enabled: bool,
    /// Whether the state stays unchanged.
    pub cancelled: bool,
}

impl Event for PlayerToggleEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A player swaps main and off hand items.
pub struct PlayerSwapHandsEvent {
    /// Affected player.
    pub player: Arc<Player>,
    /// Whether the items stay in place.
    pub cancelled: bool,
}

impl Event for PlayerSwapHandsEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A player drops items from the selected slot.
pub struct PlayerDropItemEvent {
    /// Dropping player.
    pub player: Arc<Player>,
    /// Copy of the stack about to be dropped.
    pub item: steel_registry::item_stack::ItemStack,
    /// Whether the items stay in the inventory.
    pub cancelled: bool,
}

impl Event for PlayerDropItemEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A player is about to pick up an item entity.
pub struct PlayerPickupItemEvent {
    /// Picking player.
    pub player: Arc<Player>,
    /// Copy of the stack on the ground.
    pub item: steel_registry::item_stack::ItemStack,
    /// Item entity network ID.
    pub entity_id: i32,
    /// Whether the item stays on the ground.
    pub cancelled: bool,
}

impl Event for PlayerPickupItemEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A player right-clicks an entity.
pub struct PlayerInteractEntityEvent {
    /// Interacting player.
    pub player: Arc<Player>,
    /// Clicked entity.
    pub target: super::EntityInfo,
    /// Hand used.
    pub hand: steel_utils::types::InteractionHand,
    /// Whether the interaction is ignored.
    pub cancelled: bool,
}

impl Event for PlayerInteractEntityEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A player finished editing a sign; lines can be rewritten.
pub struct SignChangeEvent {
    /// Editing player.
    pub player: Arc<Player>,
    /// Sign position.
    pub pos: steel_utils::BlockPos,
    /// Whether the front side was edited.
    pub front: bool,
    /// The four lines, already stripped of formatting codes.
    pub lines: [String; 4],
    /// Whether the sign keeps its previous text.
    pub cancelled: bool,
}

impl Event for SignChangeEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A player gains (or loses) experience points.
pub struct PlayerExpChangeEvent {
    /// Affected player.
    pub player: Arc<Player>,
    /// Points to add; may be changed.
    pub amount: i32,
}

impl Event for PlayerExpChangeEvent {}

/// A player's food level changed through hunger, regeneration or saturation.
pub struct FoodLevelChangeEvent {
    /// Affected player.
    pub player: Arc<Player>,
    /// Level before the change.
    pub previous: i32,
    /// New level (0–20).
    pub level: i32,
    /// Whether the previous level is restored.
    pub cancelled: bool,
}

impl Event for FoodLevelChangeEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}
