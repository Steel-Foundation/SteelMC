//! Item behavior implementations.
//!
//! The actual behavior registration is auto-generated from classes.json.
//! See `src/behavior/generated/items.rs` for the generated registration code.

mod air;
mod axe;
mod bed_item;
mod block_item;
mod bonemeal;
mod brush;
mod bucket;
mod compass;
mod copper_chest_events;
mod default;
mod dye;
mod dynamic_name;
mod ender_eye;
mod ender_pearl;
mod firework_rocket;
mod food_on_a_stick;
mod hoe;
mod honeycomb;
mod mace;
mod name_tag;
mod place_on_water_block_item;
mod player_head;
mod potion;
mod shield;
mod shovel;
mod sign_item;
mod spawn_egg;
mod spyglass;
mod standing_and_wall_block_item;
mod throwable_potion;
mod tipped_arrow;

mod flint_and_steel;

pub use air::AirItem;
pub use axe::AxeItem;
pub use bed_item::BedItem;
pub use block_item::{BlockItem, DoubleHighBlockItem};
pub use bonemeal::BoneMealItem;
pub use brush::BrushItem;
pub use bucket::BucketItem;
pub use compass::CompassItem;
pub use default::DefaultItemBehavior;
pub use dye::DyeItem;
pub use ender_eye::EnderEyeItem;
pub use ender_pearl::EnderPearlItem;
pub use firework_rocket::FireworkRocketItem;
pub use flint_and_steel::{FireChargeItem, FlintAndSteelItem};
pub use food_on_a_stick::FoodOnAStickItem;
pub use hoe::HoeItem;
pub use honeycomb::HoneycombItem;
pub use mace::MaceItem;
pub use name_tag::NameTagItem;
pub use place_on_water_block_item::PlaceOnWaterBlockItem;
pub use player_head::PlayerHeadItem;
pub use potion::PotionItem;
pub use shield::ShieldItem;
pub use shovel::ShovelItem;
pub use sign_item::{HangingSignItem, SignItem};
pub use standing_and_wall_block_item::StandingAndWallBlockItem;
pub use throwable_potion::{LingeringPotionItem, SplashPotionItem};
pub use tipped_arrow::TippedArrowItem;
