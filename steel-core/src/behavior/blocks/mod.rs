//! Block behavior implementations for vanilla blocks.
//!
//! The actual behavior registration is auto-generated from classes.json.
//! See `src/generated/behaviors.rs` for the generated registration code.

mod barrel_block;
mod candle_block;
mod chain_block;
mod copper_bars_block;
mod copper_chain_block;
mod crafting_table_block;
mod crop_block;
mod end_portal_frame_block;
mod farmland_block;
mod fence_block;
mod iron_bars_block;
mod redstone_torch_block;
mod rotated_pillar_block;
mod sign_block;
mod torch_block;
mod wall_block;

pub use barrel_block::BarrelBlock;
pub use candle_block::CandleBlock;
pub use chain_block::ChainBlock;
pub use copper_bars_block::WeatheringCopperBarsBlock;
pub use copper_chain_block::WeatheringCopperChainBlock;
pub use crafting_table_block::CraftingTableBlock;
pub use crop_block::CropBlock;
pub use end_portal_frame_block::EndPortalFrameBlock;
pub use farmland_block::FarmlandBlock;
pub use fence_block::FenceBlock;
pub use iron_bars_block::IronBarsBlock;
pub use redstone_torch_block::{RedstoneTorchBlock, RedstoneWallTorchBlock};
pub use rotated_pillar_block::RotatedPillarBlock;
pub use sign_block::{
    CeilingHangingSignBlock, StandingSignBlock, WallHangingSignBlock, WallSignBlock,
};
pub use torch_block::{TorchBlock, WallTorchBlock};
pub use wall_block::WallBlock;
