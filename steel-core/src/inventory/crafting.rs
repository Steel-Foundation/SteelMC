//! Crafting containers and related types.
//!
//! This module provides the `CraftingContainer` for the crafting grid,
//! `ResultContainer` for crafting output, and `CraftingInput` for recipe matching.

use std::mem;

use steel_registry::{
    item_stack::ItemStack,
    recipe::{CraftingInput, PositionedCraftingInput},
};

use crate::inventory::container::Container;

/// A container for crafting grid items.
///
/// This container holds items in a crafting grid pattern (2x2 for player inventory,
/// 3x3 for crafting table). It notifies a callback when the grid contents change,
/// allowing the crafting result to be recalculated.
pub struct CraftingContainer {
    width: usize,
    height: usize,
    items: Vec<ItemStack>,
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
    fn get_container_size(&self) -> usize {
        self.items.len()
    }

    fn get_item(&self, slot: usize) -> &ItemStack {
        &self.items[slot]
    }

    fn get_item_mut(&mut self, slot: usize) -> &mut ItemStack {
        &mut self.items[slot]
    }

    fn set_item(&mut self, slot: usize, stack: ItemStack) {
        self.items[slot] = stack;
    }

    fn set_changed(&mut self) {
        // Crafting container doesn't track dirty state itself;
        // the menu handles recipe recalculation on changes.
    }
}

/// A simple container for holding a single crafting result.
pub struct ResultContainer {
    result: ItemStack,
}

impl ResultContainer {
    /// Creates a new empty result container.
    #[must_use]
    pub fn new() -> Self {
        Self {
            result: ItemStack::empty(),
        }
    }
}

impl Default for ResultContainer {
    fn default() -> Self {
        Self::new()
    }
}

impl Container for ResultContainer {
    fn get_container_size(&self) -> usize {
        1
    }

    fn get_item(&self, _slot: usize) -> &ItemStack {
        &self.result
    }

    fn get_item_mut(&mut self, _slot: usize) -> &mut ItemStack {
        &mut self.result
    }

    fn set_item(&mut self, _slot: usize, stack: ItemStack) {
        self.result = stack;
    }

    /// Removes items from the result container.
    ///
    /// Unlike normal containers, this **always takes the entire stack**
    /// regardless of the `count` parameter. This matches Java's
    /// `ResultContainer.removeItem()` behavior which uses `takeItem()`.
    ///
    /// This ensures that right-clicking on a crafting result takes the
    /// full crafted item, not half of it.
    fn remove_item(&mut self, _slot: usize, _count: i32) -> ItemStack {
        mem::take(&mut self.result)
    }

    fn set_changed(&mut self) {
        // Result container doesn't track dirty state.
    }
}
