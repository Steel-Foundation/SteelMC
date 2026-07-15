//! Newtype for menu (container) IDs sent over the wire.

/// A menu (container) ID as used by the protocol.
///
/// `0` is reserved for the player's own inventory menu; `1..=100` are assigned
/// to opened menus by the per-player container counter, wrapping around.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MenuId(u8);

impl MenuId {
    /// The player inventory menu, which always has ID `0`.
    pub const INVENTORY: Self = Self(0);

    /// Wraps a raw container id.
    #[must_use]
    pub const fn new(id: u8) -> Self {
        Self(id)
    }

    /// Returns the raw id.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl From<MenuId> for u8 {
    fn from(id: MenuId) -> Self {
        id.0
    }
}

impl From<MenuId> for i32 {
    fn from(id: MenuId) -> Self {
        Self::from(id.0)
    }
}
