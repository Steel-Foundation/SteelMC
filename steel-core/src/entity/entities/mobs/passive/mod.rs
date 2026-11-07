//! Passive entity implementations.
/// Those mobs are passive creatures that run away when attacked by a player.
mod chicken;
mod cow;

pub use chicken::ChickenEntity;
pub use cow::CowEntity;
