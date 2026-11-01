//! Startup-installed capabilities independent of any native extension ABI.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::server::Server;
use crate::world::World;
use crate::worldgen::WorldGeneratorRegistry;

pub mod events;
pub use events::{Event, EventBus, EventHandler, EventPriority, Events};

/// Synchronous work run at the end of each normal world game tick.
pub trait WorldTickHandler: Send + Sync {
    /// Runs on the world's existing tick thread, without holding extension locks.
    fn tick(&self, world: &Arc<World>, tick_count: u64);
}

/// Synchronous work at the start of each normal server tick, before world workers.
pub trait ServerTickHandler: Send + Sync {
    /// Runs outside world workers; world lifecycle requests publish through server jobs.
    fn tick(&self, server: &Arc<Server>, tick_count: u64);
}

/// Synchronous notification before shutdown saves begin.
pub trait ServerStopHandler: Send + Sync {
    /// Runs once after tick processing has stopped.
    fn stop(&self, server: &Server);
}

/// Immutable runtime subscriptions shared by loaded worlds.
#[derive(Default)]
pub struct Extensions {
    /// Event subscriptions installed before world creation.
    pub hooks: ExtensionHooks,
}

/// Completed registration output consumed by server creation.
pub struct RegisteredExtensions {
    /// The actual generator registry populated during initialization.
    pub generators: WorldGeneratorRegistry,
    /// Event subscriptions populated during initialization.
    pub hooks: ExtensionHooks,
}

impl RegisteredExtensions {
    /// Starts registration with built-in generators and no event subscriptions.
    pub fn new_with_builtins() -> Result<Self, String> {
        Ok(Self {
            generators: WorldGeneratorRegistry::new_with_builtins()?,
            hooks: ExtensionHooks::default(),
        })
    }
}

/// Identity of one installed extension (a native extension or an embedded mod), shown by
/// `/extensions`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtensionInfo {
    /// Unique name.
    pub name: String,
    /// Extension version.
    pub version: String,
    /// Version of the extension API it was built against.
    pub api_version: String,
    /// Required extensions.
    pub depends: Vec<String>,
    /// Optional extensions loaded first when present.
    pub soft_depends: Vec<String>,
    /// Where it was loaded from, such as a library path.
    pub source: String,
}

/// Event subscriptions collected separately from registry changes during startup.
#[derive(Default)]
pub struct ExtensionHooks {
    world_ticks: Vec<Box<dyn WorldTickHandler>>,
    server_ticks: Vec<Box<dyn ServerTickHandler>>,
    server_stops: Vec<Box<dyn ServerStopHandler>>,
    stopped: AtomicBool,
    /// Gameplay event subscriptions.
    pub events: Events,
    /// Installed extensions in load order.
    pub infos: Vec<ExtensionInfo>,
}

impl ExtensionHooks {
    /// Registers a notification for orderly shutdown.
    pub fn register_server_stop(&mut self, handler: Box<dyn ServerStopHandler>) {
        self.server_stops.push(handler);
    }
    pub(crate) fn stop_server(&self, server: &Server) {
        if self.stopped.swap(true, Ordering::AcqRel) {
            return;
        }
        for handler in &self.server_stops {
            handler.stop(server);
        }
    }
    /// Registers synchronous work before world dispatch on each normal server tick.
    pub fn register_server_tick(&mut self, handler: Box<dyn ServerTickHandler>) {
        self.server_ticks.push(handler);
    }
    pub(crate) fn tick_server(&self, server: &Arc<Server>, tick_count: u64) {
        for handler in &self.server_ticks {
            handler.tick(server, tick_count);
        }
    }

    /// Registers work to run once per normal world game tick.
    pub fn register_world_tick(&mut self, handler: Box<dyn WorldTickHandler>) {
        self.world_ticks.push(handler);
    }

    pub(crate) fn tick_world(&self, world: &Arc<World>, tick_count: u64) {
        for handler in &self.world_ticks {
            handler.tick(world, tick_count);
        }
    }
}

#[cfg(test)]
mod tests;
