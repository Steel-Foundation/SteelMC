//! Item behavior implementations.
//!
//! The actual behavior registration is auto-generated from classes.json.
//! See `src/behavior/generated/items.rs` for the generated registration code.

mod axe;
mod block_item;
mod bonemeal;
mod bucket;
mod copper_chest_events;
mod default;
mod ender_eye;
mod ender_pearl;
mod food_on_a_stick;
mod hoe;
mod honeycomb;
mod mace;
mod shovel;
mod sign_item;
mod standing_and_wall_block_item;

mod flint_and_steel;

pub use axe::AxeItem;
pub use block_item::{BlockItem, DoubleHighBlockItem};
pub use bonemeal::BoneMealItem;
pub use bucket::BucketItem;
pub use default::DefaultItemBehavior;
pub use ender_eye::EnderEyeItem;
pub use ender_pearl::EnderPearlItem;
pub use flint_and_steel::{FireChargeItem, FlintAndSteelItem};
pub use food_on_a_stick::FoodOnAStickItem;
pub use hoe::HoeItem;
pub use honeycomb::HoneycombItem;
pub use mace::MaceItem;
pub use shovel::ShovelItem;
pub use sign_item::{HangingSignItem, SignItem};
pub use standing_and_wall_block_item::StandingAndWallBlockItem;
