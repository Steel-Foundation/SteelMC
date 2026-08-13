//! Single-threaded, in-memory Steel runtime for external behavior test adapters.
//!
//! This module owns the infrastructure needed to exercise production [`World`] and
//! [`Player`] behavior without starting a network server. It deliberately contains no
//! test-framework-specific types.
//!
//! [`Player`]: crate::player::Player
//! [`World`]: crate::world::World

mod connection;
mod error;
mod player;
mod world;

pub use connection::{RecordedConnectionEvent, RecordingConnection};
pub use error::TestHarnessError;
pub use player::TestPlayer;
pub use world::InMemoryWorld;

#[cfg(test)]
mod tests;
