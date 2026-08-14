//! Cooking recipe types.

use steel_utils::{DowncastType, DowncastTypeKey};

use crate::item_stack::ItemStack;

use super::{CookingRecipeBehavior, CookingRecipeKind, Ingredient, Recipe, RecipeResult};

/// A furnace smelting recipe.
#[derive(Debug)]
pub struct SmeltingRecipe {
    pub group: &'static str,
    pub ingredient: Ingredient,
    pub result: RecipeResult,
    pub experience: f32,
    pub cooking_time: i32,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete recipe type
// within the process.
unsafe impl DowncastType for SmeltingRecipe {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:recipe/smelting");
}

impl Recipe for SmeltingRecipe {
    fn group(&self) -> &str {
        self.group
    }

    fn as_cooking(&self) -> Option<&dyn CookingRecipeBehavior> {
        Some(self)
    }
}

impl CookingRecipeBehavior for SmeltingRecipe {
    fn kind(&self) -> CookingRecipeKind {
        CookingRecipeKind::Smelting
    }

    fn matches(&self, input: &ItemStack) -> bool {
        Self::matches(self, input)
    }

    fn assemble_result(&self, input_count: i32, use_input_count: bool) -> ItemStack {
        Self::assemble_result(self, input_count, use_input_count)
    }
}

impl SmeltingRecipe {
    /// Returns whether this smelting recipe accepts `input`.
    #[must_use]
    pub fn matches(&self, input: &ItemStack) -> bool {
        self.ingredient.test(input)
    }

    /// Assembles the result stack used by loot-table furnace smelting.
    #[must_use]
    pub fn assemble_result(&self, input_count: i32, use_input_count: bool) -> ItemStack {
        let count = if use_input_count { input_count } else { 1 };
        let mut result = self.result.to_item_stack();
        result.set_count(
            count
                .saturating_mul(result.count())
                .min(result.max_stack_size()),
        );
        result
    }
}

#[cfg(test)]
mod tests {
    use crate::recipe::{Ingredient, RecipeResult};
    use crate::{init_vanilla_registry, vanilla_items};

    use super::*;

    #[test]
    fn smelting_result_uses_input_count_when_requested() {
        init_vanilla_registry();
        let recipe = SmeltingRecipe {
            group: "",
            ingredient: Ingredient::Item(&vanilla_items::RAW_IRON),
            result: RecipeResult {
                item: &vanilla_items::IRON_INGOT,
                count: 1,
            },
            experience: 0.0,
            cooking_time: 200,
        };

        let result = recipe.assemble_result(3, true);

        assert!(result.is(&vanilla_items::IRON_INGOT));
        assert_eq!(result.count(), 3);
    }

    #[test]
    fn smelting_result_can_ignore_input_count() {
        init_vanilla_registry();
        let recipe = SmeltingRecipe {
            group: "",
            ingredient: Ingredient::Item(&vanilla_items::RAW_IRON),
            result: RecipeResult {
                item: &vanilla_items::IRON_INGOT,
                count: 1,
            },
            experience: 0.0,
            cooking_time: 200,
        };

        let result = recipe.assemble_result(3, false);

        assert!(result.is(&vanilla_items::IRON_INGOT));
        assert_eq!(result.count(), 1);
    }
}
