//! Shared item-frame entity behavior.

use steel_utils::Direction;

use super::Entity;

/// Vanilla `ItemFrame` behavior used by comparators.
pub trait ItemFrame: Entity {
    /// Returns the direction from the backing block toward the frame.
    fn direction(&self) -> Direction;

    /// Returns the comparator signal strength for this frame: 0 when empty,
    /// otherwise derived from the framed item's rotation.
    fn analog_output(&self) -> i32;
}
