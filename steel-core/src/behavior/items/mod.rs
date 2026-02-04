//! Item behavior implementations.
//!
//! The actual behavior registration is auto-generated from classes.json.
//! See `src/behavior/generated/items.rs` for the generated registration code.

mod block_item;
mod bucket;
mod default;
mod ender_eye;
mod shovel;
mod sign_item;
mod standing_and_wall_block_item;

pub use block_item::BlockItemBehavior;
pub use bucket::FilledBucketBehavior;
pub use default::DefaultItemBehavior;
pub use ender_eye::EnderEyeBehavior;
pub use shovel::ShovelBehaviour;
pub use sign_item::{HangingSignItemBehavior, SignItemBehavior};
pub use standing_and_wall_block_item::StandingAndWallBlockItem;
