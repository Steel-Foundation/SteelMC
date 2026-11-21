//! Passive brewing recipe data and input snapshots.

use steel_utils::{DowncastType, DowncastTypeKey};

use crate::data_component_predicate::{DataComponentPredicate, PotionsPredicate};
use crate::item_stack::ItemStack;
use crate::item_stack_template::ItemStackTemplate;
use crate::{REGISTRY, TaggedRegistryExt as _, vanilla_item_tags};

use super::{Ingredient, RecipeData, RecipeInput, RecipeMatches, RecipeProperties};

/// Brewing stand input snapshot: the potion slot stack and the reagent stack.
///
/// Vanilla passes stack references into its brewing input. Steel owns the
/// snapshot, like the other recipe inputs, so matching can run while the
/// container is mutably borrowed for the cached recipe check.
#[derive(Debug, Clone)]
pub struct BrewingInput {
    pub input: ItemStack,
    pub reagent: ItemStack,
}

impl BrewingInput {
    #[must_use]
    pub const fn new(input: ItemStack, reagent: ItemStack) -> Self {
        Self { input, reagent }
    }
}

// SAFETY: This Steel-owned key uniquely identifies a brewing matching snapshot.
unsafe impl DowncastType for BrewingInput {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:recipe_input/brewing");
}

impl RecipeInput for BrewingInput {
    fn is_empty(&self) -> bool {
        self.input.is_empty() && self.reagent.is_empty()
    }
}

/// An accepted item plus optional potion-contents restriction.
///
/// Brewing recipes use this for both the potion slot and the reagent slot, so a
/// reagent can restrict which potion results it produces.
#[derive(Debug, Clone)]
pub struct PotionIngredient {
    pub ingredient: Ingredient,
    pub potions: Option<PotionsPredicate>,
}

impl PotionIngredient {
    #[must_use]
    pub const fn new(ingredient: Ingredient, potions: Option<PotionsPredicate>) -> Self {
        Self {
            ingredient,
            potions,
        }
    }

    #[must_use]
    pub fn test(&self, stack: &ItemStack) -> bool {
        self.ingredient.test(stack)
            && self
                .potions
                .as_ref()
                .is_none_or(|potions| potions.matches(stack))
    }

    /// Returns whether `stack` may be placed in a brewing-stand potion slot.
    ///
    /// Accepts brewing recipe inputs and items tagged
    /// `minecraft:brewing_potion_inputs`, such as glass bottles.
    #[must_use]
    pub fn is_potion_input(stack: &ItemStack) -> bool {
        REGISTRY.recipes.is_brewing_input(stack.item())
            || REGISTRY.items.is_in_tag(
                stack.item(),
                &vanilla_item_tags::ItemTag::BREWING_POTION_INPUTS,
            )
    }
}

/// Brewing recipe data.
#[derive(Debug)]
pub struct BrewingRecipe {
    pub properties: RecipeProperties,
    pub input: PotionIngredient,
    pub reagent: PotionIngredient,
    pub output: ItemStackTemplate,
}

// SAFETY: This Steel-owned key uniquely identifies vanilla brewing recipe data.
unsafe impl DowncastType for BrewingRecipe {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:recipe_data/brewing");
}

impl RecipeData for BrewingRecipe {
    fn properties(&self) -> Option<&RecipeProperties> {
        Some(&self.properties)
    }
}

impl RecipeMatches<BrewingInput> for BrewingRecipe {
    fn matches(&self, input: &BrewingInput) -> bool {
        self.input.test(&input.input) && self.reagent.test(&input.reagent)
    }
}

#[cfg(test)]
mod tests {
    use crate::data_components::vanilla_components::PotionContents;
    use crate::item_stack::ItemStack;
    use crate::recipe::RecipeMatches as _;
    use crate::{REGISTRY, init_vanilla_registry, vanilla_items, vanilla_potions, vanilla_recipes};

    use super::{BrewingInput, PotionIngredient};

    #[test]
    fn brewing_property_sets_follow_the_registered_recipe_ingredients() {
        init_vanilla_registry();

        assert!(
            REGISTRY
                .recipes
                .is_brewing_reagent(&vanilla_items::BLAZE_POWDER)
        );
        assert!(!REGISTRY.recipes.is_brewing_reagent(&vanilla_items::DIRT));

        assert!(REGISTRY.recipes.is_brewing_input(&vanilla_items::POTION));
        // Glass bottles are valid potion-slot inputs only through the item tag.
        assert!(
            !REGISTRY
                .recipes
                .is_brewing_input(&vanilla_items::GLASS_BOTTLE)
        );
        assert!(PotionIngredient::is_potion_input(&ItemStack::new(
            &vanilla_items::GLASS_BOTTLE
        )));
        assert!(!PotionIngredient::is_potion_input(&ItemStack::new(
            &vanilla_items::DIRT
        )));
    }

    #[test]
    fn brewing_requires_both_the_potion_contents_and_reagent() {
        init_vanilla_registry();
        let recipe = &*vanilla_recipes::BREWING_LINGERING_POTION_AWKWARD_BLAZE_POWDER;
        let recipe = recipe.data();
        let awkward = || {
            PotionContents::create_item_stack(
                &vanilla_items::LINGERING_POTION,
                &vanilla_potions::AWKWARD,
            )
        };
        let blaze_powder = || ItemStack::new(&vanilla_items::BLAZE_POWDER);

        assert!(recipe.matches(&BrewingInput::new(awkward(), blaze_powder())));
        assert!(!recipe.matches(&BrewingInput::new(
            awkward(),
            ItemStack::new(&vanilla_items::GHAST_TEAR),
        )));
        assert!(!recipe.matches(&BrewingInput::new(
            PotionContents::create_item_stack(
                &vanilla_items::LINGERING_POTION,
                &vanilla_potions::WATER,
            ),
            blaze_powder(),
        )));
    }
}
