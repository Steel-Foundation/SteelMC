//! Minecart behavior abstraction.

use crate::entity::Entity;
use crate::world::World;
use std::sync::Arc;

/// Trait defining the movement and tick behavior of a minecart.
/// Mirrors vanilla `MinecartBehavior`.
pub trait MinecartBehavior: Send + Sync {
    /// Ticks the minecart physics and status.
    fn tick(&mut self, cart: &dyn Entity, world: &Arc<World>);
}
