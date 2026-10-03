//! Passive entity implementations.
/// Those mobs are passive creatures that run away when attacked by a player.
mod chicken;
mod cow;
mod fox;
mod pig;
mod sheep;

pub use chicken::ChickenEntity;
pub use cow::CowEntity;
pub use fox::FoxEntity;
pub use pig::PigEntity;
pub use sheep::SheepEntity;
