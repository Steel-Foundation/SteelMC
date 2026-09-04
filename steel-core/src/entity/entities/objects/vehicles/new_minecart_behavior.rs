//! Experimental new minecart behavior with spline-based movement.
//! Mirrors vanilla `NewMinecartBehavior`.

use super::minecart_behavior::MinecartBehavior;
use crate::entity::Entity;
use crate::world::World;
use std::sync::Arc;

/// Spline-based minecart behavior.
pub struct NewMinecartBehavior {}

impl NewMinecartBehavior {
    /// Creates a new `NewMinecartBehavior`.
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }
}

impl Default for NewMinecartBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl MinecartBehavior for NewMinecartBehavior {
    fn tick(&mut self, cart: &dyn Entity, world: &Arc<World>) {
        // Fallback to classic physics/logic for now.
        let mut classic = super::old_minecart_behavior::OldMinecartBehavior::new();
        classic.tick(cart, world);
    }
}
