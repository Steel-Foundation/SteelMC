//! Item behavior implementations.
//!
//! The actual behavior registration is auto-generated from classes.json.
//! See `src/behavior/generated/items.rs` for the generated registration code.

mod block_item;
mod bucket;
mod default;
mod ender_eye;
mod hoe;
mod honeycomb;
mod shovel;
mod sign_item;
mod standing_and_wall_block_item;

pub use block_item::{BlockItem, DoubleHighBlockItem};
pub use bucket::BucketItem;
pub use default::DefaultItemBehavior;
pub use ender_eye::EnderEyeItem;
pub use hoe::HoeItem;
pub use honeycomb::HoneycombItem;
pub use shovel::ShovelItem;
pub use sign_item::{HangingSignItem, SignItem};
pub use standing_and_wall_block_item::StandingAndWallBlockItem;
