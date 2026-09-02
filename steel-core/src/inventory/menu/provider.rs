//! Block menu providers shared by regular and spectator interactions.

use crate::player::Player;

/// Outcome of [`MenuProvider::create_menu`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCreation {
    /// The menu opened.
    Opened,
    /// Deferred container preparation will open the menu on a later tick.
    Deferred,
    /// Vanilla's `createMenu` would return `null`.
    Unavailable,
}

/// A block menu that players open through `BlockBehavior::get_menu_provider`.
///
/// Mirrors Vanilla `MenuProvider`. Because Steel may prepare container contents
/// across ticks, creation reports whether the menu opened, was deferred, or is
/// unavailable instead of returning the menu itself, and the provider passes the
/// display name to `Player::open_menu` directly.
pub trait MenuProvider: Send {
    /// Opens the menu for `player`, or reports why Vanilla's `createMenu` would return `null`.
    fn create_menu(self: Box<Self>, player: &Player) -> MenuCreation;
}
