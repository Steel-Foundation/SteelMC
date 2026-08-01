//! Crafting containers and related types.

use steel_registry::{
    item_stack::ItemStack,
    recipe::{CraftingInput, PositionedCraftingInput},
};
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::inventory::container::Container;

/// A container for crafting grid items.
///
/// Holds items in a crafting grid pattern (2x2 for player inventory,
/// 3x3 for crafting table).
pub struct CraftingContainer {
    width: usize,
    height: usize,
    items: Vec<ItemStack>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `CraftingContainer`.
unsafe impl DowncastType for CraftingContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/crafting");
}

impl CraftingContainer {
    /// Creates a new crafting container with the given dimensions.
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        let size = width * height;
        Self {
            width,
            height,
            items: vec![ItemStack::empty(); size],
        }
    }

    /// Returns the width of the crafting grid.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Returns the height of the crafting grid.
    #[must_use]
    pub const fn height(&self) -> usize {
        self.height
    }

    /// Creates a positioned `CraftingInput` representing the current state of the grid.
    ///
    /// The positioned input contains a trimmed version of the grid (only the
    /// bounding box of non-empty items) along with the offset from the original
    /// grid origin. This is used for recipe matching and when consuming
    /// ingredients to correctly map recipe slots back to the original crafting
    /// grid slots.
    #[must_use]
    pub fn as_positioned_input(&self) -> PositionedCraftingInput {
        CraftingInput::positioned(self.width, self.height, self.items.clone())
    }

    /// Returns a reference to the items in the grid.
    #[must_use]
    pub fn items(&self) -> &[ItemStack] {
        &self.items
    }
}

impl Container for CraftingContainer {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn set_changed(&mut self) {
        // Crafting container doesn't track dirty state itself;
        // the menu handles recipe recalculation on changes.
    }
}
