//! Player inventory management.

mod container;
mod core;
mod equipment;
mod player_handlers;

pub use container::InvalidHotbarSlot;
pub use core::PlayerInventory;
pub use equipment::EquipmentSwapResult;

#[cfg(test)]
mod tests;
