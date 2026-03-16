//! Item behavior implementations.
//!
//! The actual behavior registration is auto-generated from classes.json.
//! See `src/behavior/generated/items.rs` for the generated registration code.

mod block_item;
mod bucket;
mod default;
mod double_high_block_item;
mod ender_eye;
mod shovel;
mod sign_item;
mod standing_and_wall_block_item;

pub use block_item::BlockItemBehavior;
pub use bucket::BucketItemBehavior;
pub use default::DefaultItemBehavior;
pub use double_high_block_item::DoubleHighBlockItemBehavior;
pub use ender_eye::EnderEyeBehavior;
pub use shovel::ShovelBehavior;
pub use sign_item::{HangingSignItemBehavior, SignItemBehavior};
pub use standing_and_wall_block_item::StandingAndWallBlockItem;
