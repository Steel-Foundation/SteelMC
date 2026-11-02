//! Gameplay hooks for fluids registered with `ContentRegistry::register_fluid`.
//!
//! The host runs vanilla's spreading algorithm with the definition's parameters;
//! these hooks add what makes a fluid distinct. Every hook runs on the tick
//! thread of the world it receives.

use crate::block_behavior::types::Entity;
use crate::{BlockPos, BlockState, World};

/// Optional fluid hooks. Unimplemented hooks keep the host default.
#[stabby::stabby(checked)]
pub trait FluidBehavior: Send + Sync {
    /// An entity's bounding box is inside the fluid at `pos`. The host has already
    /// applied the displayed fluid's effect (a water-displayed fluid extinguishes).
    extern "C" fn entity_inside<'a>(
        &self,
        world: World<'a>,
        pos: BlockPos,
        state: BlockState,
        entity: Entity<'a>,
    ) {
        let _ = (world, pos, state, entity);
    }

    /// A random tick hit the fluid at `pos`; requires `randomly_ticking`.
    extern "C" fn random_tick(&self, world: World<'_>, pos: BlockPos, state: BlockState) {
        let _ = (world, pos, state);
    }

    /// The fluid is about to flow into `replaced` at `pos`. Return `true` to drop
    /// the replaced block's resources, as water does.
    extern "C" fn before_destroying_block(
        &self,
        world: World<'_>,
        pos: BlockPos,
        replaced: BlockState,
    ) -> bool {
        let _ = (world, pos, replaced);
        true
    }

    /// The fluid's block was placed or a neighbor changed. Return `false` after
    /// converting the block (for example on contact with lava), so it does not spread.
    extern "C" fn react_with_neighbors(
        &self,
        world: World<'_>,
        pos: BlockPos,
        state: BlockState,
    ) -> bool {
        let _ = (world, pos, state);
        true
    }
}

/// Owning, ABI-checked fluid hooks. The extension's destructor frees its own data.
pub type DynFluidBehavior = stabby::dynptr!(stabby::boxed::Box<dyn FluidBehavior + Send + Sync>);

#[stabby::host_api]
/// Fluid hook registration during startup.
pub mod api {
    use super::DynFluidBehavior;
    use crate::ApiResult;
    #[cfg(feature = "host")]
    use crate::host::fluid::{self as host_fluid, FluidRegistration};
    use stabby::str::Str;
    #[cfg(feature = "host")]
    use stabby::string::String as ApiString;

    /// Exclusive access to fluid behaviors before they are published.
    #[opaque]
    pub struct FluidBehaviors<'a>(&'a mut FluidRegistration<'a>);

    impl<'a> FluidBehaviors<'a> {
        /// This extension's TOML configuration.
        #[must_use]
        pub fn config(&self) -> stabby::string::String {
            self.host().config.into()
        }

        /// Attaches hooks to a fluid registered by `register_fluid`, by its source key.
        /// A fluid accepts hooks once.
        pub fn set_fluid_behavior(
            &mut self,
            fluid: Str<'_>,
            behavior: DynFluidBehavior,
        ) -> ApiResult<()> {
            host_fluid::set_fluid_behavior(self.host_mut(), fluid.as_str(), behavior)
                .map_err(ApiString::from)
                .into()
        }
    }

    impl<'a> crate::World<'a> {
        /// Schedules the fluid at `pos` to spread, as placing it normally does. Use after
        /// bulk writes, which skip fluid updates. Positions without fluid are ignored.
        pub fn schedule_fluid_tick(&self, pos: crate::BlockPos) -> ApiResult<()> {
            host_fluid::schedule_fluid_tick(self.host(), pos)
                .map_err(ApiString::from)
                .into()
        }
    }

    impl<'a> crate::Hooks<'a> {
        /// Subscribes during pre-init. Runs after block behavior registration and
        /// before extension init, with every registered fluid available.
        pub fn register_fluid_behaviors(
            &mut self,
            callback: extern "C" fn(FluidBehaviors<'_>) -> ApiResult<()>,
        ) -> ApiResult<()> {
            host_fluid::subscribe(self.host_mut(), callback)
                .map_err(ApiString::from)
                .into()
        }
    }
}
