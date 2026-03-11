//! Concrete fluid implementations: [`EmptyFluid`], [`WaterFluid`], and [`LavaFluid`].
pub mod empty;
pub mod lava;
pub mod water;

pub use empty::EmptyFluid;
pub use lava::LavaFluid;
pub use water::WaterFluid;
