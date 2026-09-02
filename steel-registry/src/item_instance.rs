//! Shared read-only view over item stacks and item stack templates.

use crate::data_components::DataComponentGetter;
use crate::items::ItemRef;

/// Read-only item identity, count, and effective components.
///
/// Mirrors Vanilla `ItemInstance`, which item predicates test so the same
/// predicate can match live stacks and the templates nested in components.
pub trait ItemInstance: DataComponentGetter {
    /// Returns the item, or air for an empty stack.
    fn item(&self) -> ItemRef;

    /// Returns the stack size.
    fn count(&self) -> i32;
}
