//! Hostile entity implementations.
//!
//! Those mobs are aggressive creatures that attack players on sight.

/// Hostile endermite entity module.
pub mod endermite;
pub use endermite::EndermiteEntity;

/// Hostile zombie entity module.
pub mod zombie;
pub use zombie::ZombieEntity;
