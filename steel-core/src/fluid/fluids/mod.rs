//! Concrete fluid implementations: [`EmptyFluid`], [`WaterFluid`], [`LavaFluid`], and
//! [`ExtensionFluid`] for registered fluids.
pub mod empty;
pub mod extension;
pub mod lava;
pub mod water;

pub use empty::EmptyFluid;
pub use extension::{ExtensionFluid, FluidHooks};
pub use lava::LavaFluid;
pub use water::WaterFluid;
