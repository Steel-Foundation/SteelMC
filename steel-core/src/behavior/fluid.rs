//! Fluid behavior registry.

use std::ops::Deref;
use std::sync::OnceLock;

use steel_registry::fluid::FluidRef;
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt};

use crate::fluid::{EmptyFluid, FluidBehavior};

/// Wrapper for the global fluid behavior registry that implements `Deref`.
pub struct FluidBehaviorLock(pub OnceLock<FluidBehaviorRegistry>);

impl Deref for FluidBehaviorLock {
    type Target = FluidBehaviorRegistry;

    fn deref(&self) -> &Self::Target {
        self.0.get().expect("Fluid behaviors not initialized")
    }
}

/// Global fluid behavior registry.
///
/// Access behaviors directly via deref: `FLUID_BEHAVIORS.get_behavior(fluid)`
pub static FLUID_BEHAVIORS: FluidBehaviorLock = FluidBehaviorLock(OnceLock::new());

/// Registry for fluid behaviors.
///
/// Created after the main registry is frozen. All fluids are initialized with
/// default behavior ([`EmptyFluid`]), then custom behaviors are registered.
pub struct FluidBehaviorRegistry {
    behaviors: Vec<Box<dyn FluidBehavior>>,
}

impl FluidBehaviorRegistry {
    /// Creates a new behavior registry with default behaviors for all fluids.
    #[must_use]
    pub fn new() -> Self {
        let fluid_count = REGISTRY.fluids.len();
        let mut behaviors: Vec<Box<dyn FluidBehavior>> = Vec::with_capacity(fluid_count);

        // Initialize all fluids with default behavior (EmptyFluid)
        for _ in 0..fluid_count {
            behaviors.push(Box::new(EmptyFluid));
        }

        Self { behaviors }
    }

    /// Sets a custom behavior for a fluid.
    ///
    /// # Panics
    /// Panics if `fluid` is not registered in the global registry.
    pub fn set_behavior(&mut self, fluid: FluidRef, behavior: Box<dyn FluidBehavior>) {
        let id = fluid.id();
        self.behaviors[id] = behavior;
    }

    /// Gets the behavior for a fluid.
    ///
    /// # Panics
    /// Panics if `fluid` is not registered in the global registry.
    #[must_use]
    pub fn get_behavior(&self, fluid: FluidRef) -> &dyn FluidBehavior {
        let id = fluid.id();
        self.behaviors[id].as_ref()
    }
}

impl Default for FluidBehaviorRegistry {
    fn default() -> Self {
        Self::new()
    }
}
