//! Concrete entity implementations.

pub mod animal;
pub mod boss;
pub mod decoration;
pub mod item;
pub mod monster;
pub mod npc;
pub mod projectile;
mod raw;
pub mod vehicle;

pub use animal::PigEntity;
pub use boss::EndCrystalEntity;
pub use decoration::{BlockDisplayEntity, ItemFrameEntity, LeashFenceKnotEntity};
pub use item::{ExperienceOrbEntity, ItemEntity};
pub use projectile::{EnderPearlEntity, FireworkRocketEntity};
pub use raw::RawEntity;
pub use vehicle::ChestMinecartEntity;
