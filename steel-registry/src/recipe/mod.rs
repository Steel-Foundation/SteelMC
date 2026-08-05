//! Recipe system for crafting and other recipe types.
//!
//! This module provides the data structures and matching logic for Minecraft recipes.
//! Currently supports crafting recipes (shaped and shapeless).

mod cooking;
mod crafting;
mod ingredient;
mod registry;

pub use cooking::SmeltingRecipe;
pub use crafting::{
    CraftingCategory, CraftingInput, CraftingRecipe, PositionedCraftingInput, RecipeResult,
    ShapedRecipe, ShapelessRecipe,
};
pub use ingredient::Ingredient;
pub use registry::RecipeRegistry;
