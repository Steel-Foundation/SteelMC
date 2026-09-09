//! Passive brewing recipe data and input snapshots.

use steel_utils::{DowncastType, DowncastTypeKey};

use crate::data_components::vanilla_components::POTION_CONTENTS;
use crate::item_stack::ItemStack;
use crate::item_stack_template::ItemStackTemplate;
use crate::potion::PotionRef;

use super::{Ingredient, RecipeData, RecipeInput, RecipeMatches, RecipeProperties};

/// An item ingredient narrowed by an optional base-potion predicate.
///
/// Matches Vanilla's `PotionIngredient` carrying a `minecraft:potion_contents`
/// predicate that only restricts the base potion.
#[derive(Debug)]
pub struct PotionIngredient {
    pub item: Ingredient,
    pub potion: Option<PotionRef>,
}

impl PotionIngredient {
    /// Tests the item ingredient, then the base potion when one is required.
    ///
    /// Custom effects and names are ignored, matching a `PotionsPredicate`
    /// that only carries `potions`.
    #[must_use]
    pub fn test(&self, stack: &ItemStack) -> bool {
        self.item.test(stack)
            && self.potion.is_none_or(|potion| {
                stack.get(POTION_CONTENTS).is_some_and(|contents| {
                    contents
                        .potion()
                        .is_some_and(|held| held.value().key == potion.key)
                })
            })
    }
}

/// One brewing stand bottle slot paired with the shared reagent slot.
#[derive(Debug)]
pub struct BrewingRecipeInput {
    pub input: ItemStack,
    pub reagent: ItemStack,
}

// SAFETY: This Steel-owned key uniquely identifies a brewing matching snapshot.
unsafe impl DowncastType for BrewingRecipeInput {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:recipe_input/brewing");
}

impl RecipeInput for BrewingRecipeInput {
    fn is_empty(&self) -> bool {
        self.input.is_empty() && self.reagent.is_empty()
    }
}

/// Brewing stand recipe data.
#[derive(Debug)]
pub struct BrewingRecipe {
    pub properties: RecipeProperties,
    pub input: PotionIngredient,
    pub reagent: PotionIngredient,
    pub result: ItemStackTemplate,
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

impl RecipeMatches<BrewingRecipeInput> for BrewingRecipe {
    fn matches(&self, input: &BrewingRecipeInput) -> bool {
        self.input.test(&input.input) && self.reagent.test(&input.reagent)
    }
}

#[cfg(test)]
mod tests {
    use super::BrewingRecipeInput;
    use crate::data_components::DataComponentPatch;
    use crate::data_components::components::PotionContents;
    use crate::data_components::vanilla_components::POTION_CONTENTS;
    use crate::item_stack::ItemStack;
    use crate::mob_effect_instance::MobEffectInstance;
    use crate::potion::Potion;
    use crate::recipe::vanilla_recipe_types;
    use crate::{REGISTRY, RegistryReference, init_vanilla_registry};
    use crate::{vanilla_items, vanilla_mob_effects, vanilla_potions};

    fn potion(potion: &'static Potion, effects: Vec<MobEffectInstance>) -> ItemStack {
        let mut patch = DataComponentPatch::new();
        patch.set(
            POTION_CONTENTS,
            PotionContents::new(Some(RegistryReference::new(potion)), None, effects, None),
        );
        ItemStack::with_count_and_patch(&vanilla_items::POTION, 1, patch)
    }

    #[test]
    fn extracted_brewing_recipes_match_their_input_and_reagent() {
        init_vanilla_registry();

        let brew = |input, reagent| {
            REGISTRY.recipes.find_match(
                &vanilla_recipe_types::BREWING,
                &BrewingRecipeInput { input, reagent },
            )
        };

        let found = brew(
            potion(&vanilla_potions::AWKWARD, Vec::new()),
            ItemStack::new(&vanilla_items::BLAZE_POWDER),
        )
        .expect("awkward potion plus blaze powder should brew");
        assert_eq!(found.data().result.item(), &*vanilla_items::POTION);
        assert!(
            found
                .data()
                .result
                .get(POTION_CONTENTS)
                .is_some_and(|contents| contents.is(&vanilla_potions::STRENGTH))
        );

        // Vanilla's `PotionsPredicate` only restricts the base potion.
        let custom_effects = vec![MobEffectInstance::new(
            vanilla_mob_effects::LUCK,
            100,
            0,
            false,
            true,
            true,
            None,
        )];
        assert!(
            brew(
                potion(&vanilla_potions::AWKWARD, custom_effects),
                ItemStack::new(&vanilla_items::BLAZE_POWDER),
            )
            .is_some()
        );

        assert!(
            brew(
                potion(&vanilla_potions::AWKWARD, Vec::new()),
                ItemStack::new(&vanilla_items::STICK),
            )
            .is_none()
        );
        assert!(
            brew(
                potion(&vanilla_potions::STRENGTH, Vec::new()),
                ItemStack::new(&vanilla_items::BLAZE_POWDER),
            )
            .is_none()
        );
        assert!(
            brew(
                ItemStack::new(&vanilla_items::POTION),
                ItemStack::new(&vanilla_items::BLAZE_POWDER),
            )
            .is_none()
        );
    }
}
