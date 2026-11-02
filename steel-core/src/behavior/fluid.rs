//! Fluid behavior registry.

use std::ops::Deref;
use std::sync::{Arc, OnceLock};

use steel_registry::fluid::FluidRef;
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt};
use steel_utils::Identifier;
use steel_utils::locks::SyncMutex;

use crate::fluid::{EmptyFluid, ExtensionFluid, FluidBehavior, FluidHooks};

type RegistrationHook = Box<dyn FnOnce(&mut FluidBehaviorRegistry) + Send>;
static REGISTRATION_HOOKS: SyncMutex<Option<Vec<RegistrationHook>>> =
    SyncMutex::new(Some(Vec::new()));

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
    /// Source fluid ids whose owner already attached hooks.
    hooked: Vec<usize>,
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

        Self {
            behaviors,
            hooked: Vec::new(),
        }
    }

    /// Sets a custom behavior for a fluid.
    ///
    /// # Panics
    /// Panics if `fluid` is not registered in the global registry.
    pub fn set_behavior(&mut self, fluid: FluidRef, behavior: Box<dyn FluidBehavior>) {
        let id = fluid.id();
        self.behaviors[id] = behavior;
    }

    /// Installs behavior edits before the registry is published. Subscribe
    /// before behaviors initialize, like block behavior registration.
    ///
    /// # Errors
    /// Returns an error if fluid behaviors were already initialized.
    pub fn on_registration(hook: impl FnOnce(&mut Self) + Send + 'static) -> Result<(), String> {
        let mut hooks = REGISTRATION_HOOKS.lock();
        let Some(hooks) = hooks.as_mut() else {
            return Err("fluid behavior registration has already finished".into());
        };
        hooks.push(Box::new(hook));
        Ok(())
    }

    pub(crate) fn run_registration_hooks(&mut self) {
        let hooks = REGISTRATION_HOOKS.lock().take();
        if let Some(hooks) = hooks {
            for hook in hooks {
                hook(self);
            }
        }
    }

    /// Gives every registered fluid its host-run behavior, without hooks.
    pub(crate) fn register_client_fluids(&mut self) {
        for &fluid in REGISTRY.client_content.fluids() {
            self.set_behavior(fluid.source, Box::new(ExtensionFluid::new(fluid, None)));
            self.set_behavior(fluid.flowing, Box::new(ExtensionFluid::new(fluid, None)));
        }
    }

    /// Attaches gameplay hooks to a registered fluid, given by its source key.
    /// Hooks can be attached once, so a second extension cannot replace them.
    ///
    /// # Errors
    /// Returns an error if the key does not name a registered fluid or the fluid
    /// already has hooks.
    pub fn set_fluid_hooks(
        &mut self,
        source: &Identifier,
        hooks: Arc<dyn FluidHooks>,
    ) -> Result<(), String> {
        let fluid = REGISTRY
            .fluids
            .by_key(source)
            .and_then(|fluid| REGISTRY.client_content.fluid(fluid))
            .ok_or_else(|| format!("{source} is not a registered extension fluid"))?;
        let id = fluid.source.id();
        if self.hooked.contains(&id) {
            return Err(format!("{source} already has fluid hooks"));
        }
        self.hooked.push(id);
        self.set_behavior(
            fluid.source,
            Box::new(ExtensionFluid::new(fluid, Some(Arc::clone(&hooks)))),
        );
        self.set_behavior(
            fluid.flowing,
            Box::new(ExtensionFluid::new(fluid, Some(hooks))),
        );
        Ok(())
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
