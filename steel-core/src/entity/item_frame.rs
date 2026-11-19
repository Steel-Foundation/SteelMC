//! Shared item-frame entity behavior.

use super::HangingEntity;
use steel_registry::item_stack::ItemStack;

/// Vanilla `ItemFrame` behavior used by comparators.
pub trait ItemFrame: HangingEntity {
    /// Snapshot of the single displayed item.
    fn item(&self) -> ItemStack;
    /// Returns the comparator signal strength for this frame: 0 when empty,
    /// otherwise derived from the framed item's rotation.
    fn analog_output(&self) -> i32;
}
