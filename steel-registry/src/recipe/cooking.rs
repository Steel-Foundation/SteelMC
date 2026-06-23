//! Cooking recipe types.

use steel_utils::Identifier;

use crate::item_stack::ItemStack;

use super::{Ingredient, RecipeResult};

/// A furnace smelting recipe.
#[derive(Debug)]
pub struct SmeltingRecipe {
    pub id: Identifier,
    pub ingredient: Ingredient,
    pub result: RecipeResult,
    pub experience: f32,
    pub cooking_time: i32,
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
    use steel_utils::Identifier;

    use crate::recipe::{Ingredient, RecipeResult};
    use crate::{test_support::init_test_registry, vanilla_items};

    use super::*;

    #[test]
    fn smelting_result_uses_input_count_when_requested() {
        init_test_registry();
        let recipe = SmeltingRecipe {
            id: Identifier::vanilla_static("test"),
            ingredient: Ingredient::Item(&vanilla_items::ITEMS.raw_iron),
            result: RecipeResult {
                item: &vanilla_items::ITEMS.iron_ingot,
                count: 1,
            },
            experience: 0.0,
            cooking_time: 200,
        };

        let result = recipe.assemble_result(3, true);

        assert!(result.is(&vanilla_items::ITEMS.iron_ingot));
        assert_eq!(result.count(), 3);
    }

    #[test]
    fn smelting_result_can_ignore_input_count() {
        init_test_registry();
        let recipe = SmeltingRecipe {
            id: Identifier::vanilla_static("test"),
            ingredient: Ingredient::Item(&vanilla_items::ITEMS.raw_iron),
            result: RecipeResult {
                item: &vanilla_items::ITEMS.iron_ingot,
                count: 1,
            },
            experience: 0.0,
            cooking_time: 200,
        };

        let result = recipe.assemble_result(3, false);

        assert!(result.is(&vanilla_items::ITEMS.iron_ingot));
        assert_eq!(result.count(), 1);
    }
}
