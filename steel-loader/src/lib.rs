//! Steel Mod Loader - Native mod loading for SteelMC.

pub mod api;
pub mod discovery;
pub mod loader;
pub mod manifest;
pub mod resolver;

pub use api::{ModContext, SteelModInitFn, SteelModShutdownFn};
pub use discovery::{DiscoveredMod, ModDiscovery, ModSource};
pub use loader::{LoadedMod, ModLoader, ModLoaderError};
pub use manifest::{ModEnvironment, ModManifest};
pub use resolver::{DependencyResolver, ResolutionError};

#[cfg(test)]
mod tests;
