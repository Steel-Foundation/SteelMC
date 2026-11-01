//! Container menu events.

use std::sync::Arc;

use steel_protocol::packets::game::ClickType;
use text_components::TextComponent;

use super::Event;
use crate::player::Player;

/// A container menu is about to open for a player.
pub struct InventoryOpenEvent {
    /// Viewing player.
    pub player: Arc<Player>,
    /// Menu title.
    pub title: TextComponent,
    /// Whether the menu stays closed.
    pub cancelled: bool,
}

impl Event for InventoryOpenEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A click inside an open container menu or the player inventory.
pub struct InventoryClickEvent {
    /// Clicking player.
    pub player: Arc<Player>,
    /// Container ID of the clicked menu (0 is the player inventory).
    pub container_id: i32,
    /// Clicked slot; -999 outside the window.
    pub slot: i16,
    /// Mouse button or hotbar key.
    pub button: i8,
    /// Click kind.
    pub click: ClickType,
    /// Whether nothing moves; the client is resynchronized.
    pub cancelled: bool,
}

impl Event for InventoryClickEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A container menu closed.
pub struct InventoryCloseEvent {
    /// Viewing player.
    pub player: Arc<Player>,
    /// Container ID of the closed menu.
    pub container_id: u8,
}

impl Event for InventoryCloseEvent {}
