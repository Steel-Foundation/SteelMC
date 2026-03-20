//! Container trait for anything that holds items.
//!
//! Containers are the base abstraction for anything that can hold items,
//! including player inventories, chests, barrels, furnaces, etc.

use std::mem;
use std::ptr;

use enum_dispatch::enum_dispatch;
use steel_registry::item_stack::ItemStack;

/// Default distance buffer for container interaction range checks.
pub const DEFAULT_DISTANCE_BUFFER: f32 = 4.0;

/// Something that contains items.
/// I also use container interchangeably with inventory as they mean approximately the same thing.
/// But inventory could also refer to the player's inventory.
/// Example: `PlayerInventory`, Chest, Temporary Crafting Table
#[enum_dispatch]
pub trait Container {
    /// Returns the number of slots in this container.
    fn get_container_size(&self) -> usize;

    /// Returns true if all slots in this container are empty.
    fn is_empty(&self) -> bool {
        for i in 0..self.get_container_size() {
            if !self.get_item(i).is_empty() {
                return false;
            }
        }
        true
    }

    /// Returns a reference to the item in the specified slot.
    fn get_item(&self, slot: usize) -> &ItemStack;

    /// Returns a mutable reference to the item in the specified slot.
    fn get_item_mut(&mut self, slot: usize) -> &mut ItemStack;

    /// Sets the item in the specified slot.
    fn set_item(&mut self, slot: usize, stack: ItemStack);

    /// Removes up to `count` items from the specified slot and returns them.
    fn remove_item(&mut self, slot: usize, count: i32) -> ItemStack {
        let item = self.get_item_mut(slot);
        if item.is_empty() || count <= 0 {
            return ItemStack::empty();
        }
        item.split(count)
    }

    /// Removes the item from the specified slot without triggering updates.
    fn remove_item_no_update(&mut self, slot: usize) -> ItemStack {
        mem::take(self.get_item_mut(slot))
    }

    /// Returns the maximum stack size for this container.
    fn get_max_stack_size(&self) -> i32 {
        99
    }

    /// Returns the maximum stack size for a specific item in this container.
    ///
    /// Takes the minimum of the container's max stack size and the item's max stack size.
    /// Based on Java's `Container.getMaxStackSize(ItemStack)`.
    fn get_max_stack_size_for_item(&self, item: &ItemStack) -> i32 {
        self.get_max_stack_size().min(item.max_stack_size())
    }

    /// Marks this container as changed (dirty) for saving/syncing.
    fn set_changed(&mut self);

    /// Returns true if the player can still interact with this container.
    ///
    /// This is used to validate that:
    /// - The container still exists (e.g., chest block hasn't been destroyed)
    /// - The player is within interaction range
    /// - Any other conditions for valid interaction
    ///
    /// The default implementation always returns true (e.g., for player inventory).
    /// Block-based containers should override this to check block existence and distance.
    ///
    /// Based on Java's `Container.stillValid(Player)`.
    fn still_valid(&self) -> bool {
        true
    }

    /// Returns true if the specified item can be placed in the specified slot.
    fn can_place_item(&self, _slot: usize, _stack: &ItemStack) -> bool {
        true
    }

    /// Returns true if the specified item can be taken from the specified slot.
    fn can_take_item(&self, _slot: usize, _stack: &ItemStack) -> bool {
        true
    }

    /// Clears all items from this container.
    fn clear_content(&mut self) -> i32 {
        let mut count = 0;
        for i in 0..self.get_container_size() {
            let item = self.get_item_mut(i);
            count += item.count;
            *item = ItemStack::empty();
        }
        count
    }

    /// Clears all items from this container.
    fn clear_content_matching(&mut self, predicate: &mut dyn FnMut(&mut ItemStack) -> bool) -> i32 {
        let mut count = 0;
        for i in 0..self.get_container_size() {
            let item = self.get_item_mut(i);
            if predicate(item) {
                count += item.count;
                *item = ItemStack::empty();
            }
        }
        count
    }

    /// Returns mutable references to `N` disjoint slots.
    ///
    /// # Panics
    ///
    /// Panics if any index is out of bounds or if any two indices are equal.
    fn with_indices<const N: usize>(&mut self, indices: [usize; N]) -> [&mut ItemStack; N]
    where
        Self: Sized,
    {
        with_indices(self, indices)
    }

    /// Tries to add an item to the container.
    ///
    /// First tries to stack with existing matching items, then tries empty slots.
    /// Returns true if the entire stack was added, false if some or all couldn't fit.
    /// The passed stack is modified to contain any remaining items.
    ///
    /// Based on Java's `Inventory.add(ItemStack)`.
    fn add(&mut self, stack: &mut ItemStack) -> bool {
        if stack.is_empty() {
            return true;
        }

        let size = self.get_container_size();
        let max_size = self.get_max_stack_size_for_item(stack);

        // First pass: try to stack with existing items
        if stack.is_stackable() {
            for slot in 0..size {
                if stack.is_empty() {
                    return true;
                }
                let existing = self.get_item_mut(slot);
                if !existing.is_empty() && ItemStack::is_same_item_same_components(existing, stack)
                {
                    let space = max_size - existing.count();
                    if space > 0 {
                        let to_add = stack.count().min(space);
                        existing.grow(to_add);
                        stack.shrink(to_add);
                    }
                }
            }
        }

        // Second pass: try empty slots
        for slot in 0..size {
            if stack.is_empty() {
                return true;
            }
            if self.get_item(slot).is_empty() && self.can_place_item(slot, stack) {
                let to_place = stack.count().min(max_size);
                self.set_item(slot, stack.split(to_place));
            }
        }

        stack.is_empty()
    }
}

/// Returns mutable references to `N` disjoint slots in a container.
///
/// # Panics
///
/// Panics if any index is out of bounds or if any two indices are equal.
pub fn with_indices<const N: usize>(
    container: &mut (impl Container + ?Sized),
    indices: [usize; N],
) -> [&mut ItemStack; N] {
    let size = container.get_container_size();
    for i in 0..N {
        assert!(
            indices[i] < size,
            "with_indices: index {} out of bounds (container size {})",
            indices[i],
            size,
        );
        for j in (i + 1)..N {
            assert!(
                indices[i] != indices[j],
                "with_indices: duplicate index {}",
                indices[i],
            );
        }
    }

    let mut ptrs = [ptr::null_mut::<ItemStack>(); N];
    for i in 0..N {
        ptrs[i] = ptr::from_mut(container.get_item_mut(indices[i]));
    }
    // SAFETY: All indices are verified unique and in-bounds above. Each call to
    // `get_item_mut` returns a pointer to a distinct slot, so the resulting
    // mutable references do not alias.
    ptrs.map(|ptr| unsafe { &mut *ptr })
}

/// Calculates the redstone comparator signal strength (0-15) from a container.
///
/// Based on Java's `AbstractContainerMenu.getRedstoneSignalFromContainer`.
/// The signal is proportional to how full the container is:
/// - 0 = empty
/// - 1-14 = partially filled (linear interpolation)
/// - 15 = completely full
///
/// # Arguments
/// * `container` - The container to calculate the signal for
///
/// # Returns
/// Signal strength from 0 to 15
#[must_use]
pub fn calculate_redstone_signal_from_container(container: &dyn Container) -> i32 {
    let size = container.get_container_size();
    if size == 0 {
        return 0;
    }

    let mut total_percent: f32 = 0.0;

    for i in 0..size {
        let item = container.get_item(i);
        if !item.is_empty() {
            let max_stack = container.get_max_stack_size_for_item(item);
            total_percent += item.count() as f32 / max_stack as f32;
        }
    }

    total_percent /= size as f32;

    // Lerp from 0 to 15 based on fullness
    // Equivalent to Java's Mth.lerpDiscrete(totalPercent, 0, 15)
    (total_percent * 15.0).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestContainer {
        items: Vec<ItemStack>,
    }

    impl TestContainer {
        fn new(size: usize) -> Self {
            Self {
                items: (0..size).map(|_| ItemStack::empty()).collect(),
            }
        }
    }

    impl Container for TestContainer {
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

        fn set_changed(&mut self) {}
    }

    #[test]
    fn test_with_indices_disjoint() {
        let mut container = TestContainer::new(4);
        let [a, b] = with_indices(&mut container, [1, 3]);
        a.count = 10;
        b.count = 20;
        assert_eq!(container.items[1].count, 10);
        assert_eq!(container.items[3].count, 20);
        // Untouched slots remain at 0
        assert_eq!(container.items[0].count, 0);
        assert_eq!(container.items[2].count, 0);
    }

    #[test]
    fn test_with_indices_single() {
        let mut container = TestContainer::new(4);
        let [a] = with_indices(&mut container, [2]);
        a.count = 42;
        assert_eq!(container.items[2].count, 42);
    }

    #[test]
    fn test_with_indices_empty() {
        let mut container = TestContainer::new(4);
        let [] = with_indices(&mut container, []);
    }

    #[test]
    #[should_panic(expected = "duplicate index")]
    fn test_with_indices_duplicate_panics() {
        let mut container = TestContainer::new(4);
        let _ = with_indices(&mut container, [1, 1]);
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn test_with_indices_out_of_bounds_panics() {
        let mut container = TestContainer::new(4);
        let _ = with_indices(&mut container, [5]);
    }

    /// Verify the compiler prevents holding a `get_item_mut` reference while
    /// calling `with_indices` on the same container. Uncomment the body to
    /// see the expected borrow-checker error:
    ///
    /// ```compile_fail
    /// use steel_core::inventory::container::{Container, with_indices};
    /// # struct C { items: Vec<steel_registry::item_stack::ItemStack> }
    /// # impl Container for C {
    /// #     fn get_container_size(&self) -> usize { self.items.len() }
    /// #     fn get_item(&self, s: usize) -> &steel_registry::item_stack::ItemStack { &self.items[s] }
    /// #     fn get_item_mut(&mut self, s: usize) -> &mut steel_registry::item_stack::ItemStack { &mut self.items[s] }
    /// #     fn set_item(&mut self, s: usize, v: steel_registry::item_stack::ItemStack) { self.items[s] = v; }
    /// #     fn set_changed(&mut self) {}
    /// # }
    /// fn fails(c: &mut C) {
    ///     let held = c.get_item_mut(0);
    ///     let [a] = with_indices(c, [1]); // ERROR: c already borrowed
    ///     held.count = 1;
    /// }
    /// ```
    fn _compile_fail_docs_only() {}
}
