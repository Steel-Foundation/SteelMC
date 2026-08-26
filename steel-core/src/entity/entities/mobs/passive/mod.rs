//! Passive entity implementations.
/// Those mobs are passive creatures that run away when attacked by a player.
mod chicken;
mod cow;
mod pig;
mod sheep;

pub use chicken::ChickenEntity;
pub use cow::CowEntity;
pub use pig::PigEntity;
pub use sheep::SheepEntity;
