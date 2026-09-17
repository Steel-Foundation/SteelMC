//! Passive single-item recipe data and input snapshots.

use steel_utils::{DowncastType, DowncastTypeKey};

use crate::item_stack::ItemStack;
use crate::item_stack_template::ItemStackTemplate;

use super::{Ingredient, RecipeData, RecipeInput, RecipeMatches, RecipeProperties};

/// Input shared by cooking and stonecutting recipe types.
#[derive(Debug, Clone)]
pub struct SingleItemRecipeInput {
    pub item: ItemStack,
}

impl SingleItemRecipeInput {
    #[must_use]
    pub const fn new(item: ItemStack) -> Self {
        Self { item }
    }
}

// SAFETY: This Steel-owned key uniquely identifies a single-item matching snapshot.
unsafe impl DowncastType for SingleItemRecipeInput {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:recipe_input/single_item");
}

impl RecipeInput for SingleItemRecipeInput {
    fn is_empty(&self) -> bool {
        self.item.is_empty()
    }
}

/// Data shared by smelting, blasting, smoking, and campfire cooking.
#[derive(Debug)]
pub struct CookingRecipe {
    pub properties: RecipeProperties,
    pub ingredient: Ingredient,
    pub result: ItemStackTemplate,
    pub experience: f32,
    pub cooking_time: i32,
}

// SAFETY: This Steel-owned key uniquely identifies vanilla cooking recipe data.
unsafe impl DowncastType for CookingRecipe {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:recipe_data/cooking");
}

impl RecipeData for CookingRecipe {
    fn properties(&self) -> Option<&RecipeProperties> {
        Some(&self.properties)
    }
}

/// Stonecutter recipe data.
#[derive(Debug)]
pub struct StonecuttingRecipe {
    pub properties: RecipeProperties,
    pub ingredient: Ingredient,
    pub result: ItemStackTemplate,
}

// SAFETY: This Steel-owned key uniquely identifies vanilla stonecutting recipe data.
unsafe impl DowncastType for StonecuttingRecipe {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:recipe_data/stonecutting");
}

impl RecipeData for StonecuttingRecipe {
    fn properties(&self) -> Option<&RecipeProperties> {
        Some(&self.properties)
    }
}

impl RecipeMatches<SingleItemRecipeInput> for CookingRecipe {
    fn matches(&self, input: &SingleItemRecipeInput) -> bool {
        self.ingredient.test(&input.item)
    }
}

impl RecipeMatches<SingleItemRecipeInput> for StonecuttingRecipe {
    fn matches(&self, input: &SingleItemRecipeInput) -> bool {
        self.ingredient.test(&input.item)
    }
}
