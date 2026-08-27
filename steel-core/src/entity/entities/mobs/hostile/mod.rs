//! Hostile entity implementations.
//!
//! Those mobs are aggressive creatures that attack players on sight.

mod blaze;
/// Hostile endermite entity module.
pub mod endermite;

pub use blaze::BlazeEntity;
pub use endermite::EndermiteEntity;
