//! Default item behavior implementation.

use crate::behavior::ItemBehavior;

/// Vanilla's base item behavior.
///
/// Shared component-driven behavior belongs on [`ItemBehavior`]'s default
/// methods so specialized items inherit it too.
pub struct DefaultItemBehavior;

impl ItemBehavior for DefaultItemBehavior {}
