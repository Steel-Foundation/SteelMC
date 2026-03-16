//! Block behavior implementations for vanilla blocks.
//!
//! The actual behavior registration is auto-generated from classes.json.
//! See `src/generated/behaviors.rs` for the generated registration code.

mod building;
mod container;
mod decoration;
mod farming;
mod fluid;
mod portal;
mod redstone;

pub use building::{
    FenceBlock, RotatedPillarBlock, WeatherState, WeatheringCopper, WeatheringCopperFullBlock,
};
pub use container::{BarrelBlock, CraftingTableBlock};
pub use decoration::{
    CandleBlock, CeilingHangingSignBlock, StandingSignBlock, TorchBlock, WallHangingSignBlock,
    WallSignBlock, WallTorchBlock,
};
pub use farming::{CactusBlock, CactusFlowerBlock, CropBlock, FarmlandBlock};
pub use fluid::LiquidBlock;
pub use portal::{EndPortalFrameBlock, FireBlock, NetherPortalBlock};
pub use redstone::{ButtonBlock, RedstoneTorchBlock, RedstoneWallTorchBlock};
