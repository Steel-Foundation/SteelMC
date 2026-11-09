//! Vehicle entity implementations.

mod abstract_minecart;
mod chest_minecart;
mod minecart;
mod minecart_behavior;
mod vehicle_entity;

pub use abstract_minecart::{AbstractMinecart, AbstractMinecartBase};
pub(crate) use abstract_minecart::{CreateMinecartError, create_minecart};
pub use chest_minecart::ChestMinecartEntity;
pub use minecart::MinecartEntity;
pub use minecart_behavior::MinecartBehavior;
pub use vehicle_entity::VehicleEntity;

#[cfg(test)]
#[path = "tests/support.rs"]
mod test_support;
