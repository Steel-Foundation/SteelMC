//! Item behavior implementations.
//!
//! The actual behavior registration is auto-generated from classes.json.
//! See `src/behavior/generated/items.rs` for the generated registration code.

mod block_item;
mod bucket;
mod default;
mod ender_eye;

pub use block_item::BlockItemBehavior;
pub use bucket::FilledBucketBehavior;
pub use default::DefaultItemBehavior;
pub use ender_eye::EnderEyeBehavior;
