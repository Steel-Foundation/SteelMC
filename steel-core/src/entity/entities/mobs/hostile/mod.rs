//! Hostile entity implementations.
//!
//! Those mobs are aggressive creatures that attack players on sight.

/// Hostile blaze entity module.
pub mod blaze;
/// Hostile enderman entity module.
pub mod enderman;
/// Hostile endermite entity module.
pub mod endermite;
pub use blaze::BlazeEntity;
pub use enderman::EndermanEntity;
pub use endermite::EndermiteEntity;
