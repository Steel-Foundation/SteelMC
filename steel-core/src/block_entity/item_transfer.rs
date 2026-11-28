//! Container-to-container item transfer shared by hoppers (and later droppers and
//! dispensers).
//!
//! Mirrors the static helpers of Vanilla `HopperBlockEntity`: `getSlots`,
//! `addItem`, `tryMoveInItem`, `canPlaceItemInContainer` and
//! `canTakeItemFromContainer`.

use std::borrow::Cow;

use steel_registry::blocks::properties::Direction;
use steel_registry::item_stack::ItemStack;

use crate::inventory::container::Container;

/// Vanilla `HopperBlockEntity.getSlots`: the slots exposed on `direction`, or every
/// slot when the container has no face-specific view or no direction is given.
pub(crate) fn exposed_slots(
    container: &dyn Container,
    direction: Option<Direction>,
) -> Cow<'static, [usize]> {
    match direction.and_then(|d| container.slots_for_face(d)) {
        Some(slots) => Cow::Borrowed(slots),
        None => Cow::Owned((0..container.get_container_size()).collect()),
    }
}

/// Vanilla `HopperBlockEntity.isFullContainer`.
pub(crate) fn is_full_container(container: &dyn Container, direction: Direction) -> bool {
    exposed_slots(container, Some(direction))
        .iter()
        .all(|&slot| {
            let stack = container.get_item(slot);
            !stack.is_empty() && stack.count() >= stack.max_stack_size()
        })
}

/// Vanilla `HopperBlockEntity.canTakeItemFromContainer`.
pub(crate) fn can_take_from(
    into: &dyn Container,
    from: &dyn Container,
    slot: usize,
    stack: &ItemStack,
    direction: Direction,
) -> bool {
    from.can_take_item(into, slot, stack) && from.can_take_item_through_face(slot, stack, direction)
}

/// Vanilla `HopperBlockEntity.canPlaceItemInContainer`.
///
/// A `None` direction skips the face check, where Vanilla passes `null` through.
fn can_place(
    container: &dyn Container,
    stack: &ItemStack,
    slot: usize,
    direction: Option<Direction>,
) -> bool {
    container.can_place_item(slot, stack)
        && direction.is_none_or(|d| container.can_place_item_through_face(slot, stack, d))
}

/// Vanilla `HopperBlockEntity.tryMoveInItem`: places or merges `stack` into `slot` and
/// returns what did not fit.
fn try_move_in_item(
    to: &mut dyn Container,
    mut stack: ItemStack,
    slot: usize,
    direction: Option<Direction>,
) -> ItemStack {
    if !can_place(to, &stack, slot, direction) {
        return stack;
    }
    let current = to.get_item(slot).clone();
    if current.is_empty() {
        to.set_item(slot, stack);
        to.set_changed();
        return ItemStack::empty();
    }
    if current.count() <= current.max_stack_size()
        && ItemStack::is_same_item_same_components(&current, &stack)
    {
        let space = stack.max_stack_size() - current.count();
        let count = stack.count().min(space);
        if count > 0 {
            stack.set_count(stack.count() - count);
            to.get_item_mut(slot).set_count(current.count() + count);
            to.set_changed();
        }
    }
    stack
}

/// Vanilla `HopperBlockEntity.addItem(from, container, stack, direction)`: fills
/// every eligible slot in order and returns the remainder.
pub(crate) fn add_item(
    to: &mut dyn Container,
    mut stack: ItemStack,
    direction: Option<Direction>,
) -> ItemStack {
    let slots = exposed_slots(&*to, direction);
    for &slot in slots.iter() {
        if stack.is_empty() {
            break;
        }
        stack = try_move_in_item(to, stack, slot, direction);
    }
    stack
}

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_items};
    use steel_utils::{DowncastType, DowncastTypeKey};

    use super::*;

    struct TestContainer {
        items: Vec<ItemStack>,
    }

    impl TestContainer {
        fn new(size: usize) -> Self {
            Self {
                items: vec![ItemStack::empty(); size],
            }
        }
    }

    // SAFETY: Test-only key, unique to this test container.
    unsafe impl DowncastType for TestContainer {
        const TYPE_KEY: DowncastTypeKey =
            DowncastTypeKey::new("steel:test/item_transfer_container");
    }

    impl Container for TestContainer {
        fn items(&self) -> &[ItemStack] {
            &self.items
        }

        fn items_mut(&mut self) -> &mut [ItemStack] {
            &mut self.items
        }

        fn get_container_size(&self) -> usize {
            self.items.len()
        }

        fn set_changed(&mut self) {}
    }

    #[test]
    fn add_item_fills_slots_in_order_like_vanilla() {
        init_vanilla_registry();
        let mut container = TestContainer::new(3);
        container.set_item(1, ItemStack::with_count(&vanilla_items::STONE, 10));

        let remainder = add_item(
            &mut container,
            ItemStack::with_count(&vanilla_items::STONE, 5),
            None,
        );

        // Vanilla iterates slots in index order: the empty slot 0 takes the whole stack first.
        assert!(remainder.is_empty());
        assert_eq!(container.get_item(0).count(), 5);
        assert_eq!(container.get_item(1).count(), 10);
    }

    #[test]
    fn add_item_returns_remainder_when_full() {
        init_vanilla_registry();
        let mut container = TestContainer::new(1);
        container.set_item(0, ItemStack::with_count(&vanilla_items::STONE, 64));

        let remainder = add_item(
            &mut container,
            ItemStack::with_count(&vanilla_items::STONE, 3),
            None,
        );

        assert_eq!(remainder.count(), 3);
        assert_eq!(container.get_item(0).count(), 64);
    }

    #[test]
    fn full_container_requires_every_slot_at_max() {
        init_vanilla_registry();
        let mut container = TestContainer::new(2);
        container.set_item(0, ItemStack::with_count(&vanilla_items::STONE, 64));
        assert!(!is_full_container(&container, Direction::Down));

        container.set_item(1, ItemStack::with_count(&vanilla_items::STONE, 64));
        assert!(is_full_container(&container, Direction::Down));
    }
}
