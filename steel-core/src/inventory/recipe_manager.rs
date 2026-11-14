//! Recipe matching and crafting-grid interpretation.

use steel_registry::data_components::vanilla_components::{BANNER_PATTERNS, WRITTEN_BOOK_CONTENT};
use steel_registry::recipe::{
    CraftingInput, CraftingRecipe, PositionedCraftingInput, TypedRecipeRef, vanilla_recipe_types,
};
use steel_registry::{REGISTRY, item_stack::ItemStack};

use crate::inventory::container::CraftingContainer;

use super::container::Container;

/// Typed reference to one registered crafting recipe.
pub type CraftingRecipeRef = TypedRecipeRef<CraftingRecipe, CraftingInput>;

/// Updates a result container from the first matching crafting recipe.
pub fn slot_changed_crafting_grid<R: Container>(crafting: &CraftingContainer, result: &mut R) {
    result.set_item(
        0,
        assemble_for_container(crafting).unwrap_or_else(ItemStack::empty),
    );
}

/// Finds the first crafting recipe in deterministic registry order.
#[must_use]
pub fn find_recipe(crafting: &CraftingContainer) -> Option<CraftingRecipeRef> {
    let positioned = crafting.as_positioned_input();
    REGISTRY
        .recipes
        .find_match(&vanilla_recipe_types::CRAFTING, &positioned.input)
}

/// Finds and interprets the output for the current crafting container.
#[must_use]
pub fn assemble_for_container(crafting: &CraftingContainer) -> Option<ItemStack> {
    let positioned = crafting.as_positioned_input();
    let recipe = find_recipe(crafting)?;
    Some(recipe.data().assemble(&positioned.input))
}

/// Returns crafting remainders and the positioned input used to map them back
/// to the original grid.
#[must_use]
pub fn get_remaining_items(
    crafting: &CraftingContainer,
) -> Option<(Vec<ItemStack>, PositionedCraftingInput)> {
    let positioned = crafting.as_positioned_input();
    let recipe = find_recipe(crafting)?;
    let mut remainders: Vec<_> = positioned
        .input
        .items
        .iter()
        .map(|stack| {
            if stack.is_empty() {
                ItemStack::empty()
            } else {
                stack.item().get_crafting_remainder()
            }
        })
        .collect();

    match recipe.data() {
        CraftingRecipe::BannerDuplicate(_) => {
            for (slot, stack) in positioned.input.items.iter().enumerate() {
                if remainders[slot].is_empty()
                    && stack
                        .get(BANNER_PATTERNS)
                        .is_some_and(|patterns| !patterns.layers().is_empty())
                {
                    remainders[slot] = stack.copy_with_count(1);
                }
            }
        }
        CraftingRecipe::BookCloning(_) => {
            for (slot, stack) in positioned.input.items.iter().enumerate() {
                if remainders[slot].is_empty() && stack.has(WRITTEN_BOOK_CONTENT) {
                    remainders[slot] = stack.copy_with_count(1);
                    break;
                }
            }
        }
        _ => {}
    }

    Some((remainders, positioned))
}

#[cfg(test)]
mod tests {
    use steel_registry::data_components::vanilla_components::DYE;
    use steel_registry::recipe::{CraftingInput, vanilla_recipe_types};
    use steel_registry::{DyeColor, REGISTRY, init_vanilla_registry, vanilla_items};

    use steel_registry::item_stack::ItemStack;

    #[test]
    fn external_crafting_interpreter_assembles_a_generated_dye_recipe() {
        init_vanilla_registry();
        let input = CraftingInput::new(
            2,
            1,
            vec![
                ItemStack::new(&vanilla_items::RED_DYE),
                ItemStack::new(&vanilla_items::BLUE_DYE),
            ],
        );
        let Some(recipe) = REGISTRY
            .recipes
            .find_match(&vanilla_recipe_types::CRAFTING, &input)
        else {
            panic!("red and blue dye should match the purple dye recipe");
        };

        let result = recipe.data().assemble(&input);

        assert!(result.is(&vanilla_items::PURPLE_DYE));
        assert_eq!(result.get(DYE), Some(&DyeColor::Purple));
    }
}
