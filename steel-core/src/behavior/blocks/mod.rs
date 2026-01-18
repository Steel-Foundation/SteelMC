//! Block behavior implementations for vanilla blocks.
//!
//! The actual behavior registration is auto-generated from classes.json.
//! See `src/generated/behaviors.rs` for the generated registration code.

mod crafting_table_block;
mod crop_block;
mod end_portal_frame_block;
mod farmland_block;
mod fence_block;
mod rotated_pillar_block;

pub use crafting_table_block::CraftingTableBlock;
pub use crop_block::CropBlock;
pub use end_portal_frame_block::EndPortalFrameBlock;
pub use farmland_block::FarmlandBlock;
pub use fence_block::FenceBlock;
pub use rotated_pillar_block::RotatedPillarBlock;
