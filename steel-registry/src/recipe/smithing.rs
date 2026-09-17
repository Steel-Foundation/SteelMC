//! Passive smithing recipe data and input snapshots.

use steel_utils::{DowncastType, DowncastTypeKey};

use crate::item_stack::ItemStack;
use crate::item_stack_template::ItemStackTemplate;
use crate::trim_pattern::TrimPatternRef;

use super::{Ingredient, RecipeData, RecipeInput, RecipeMatches, RecipeProperties};

#[derive(Debug)]
pub struct SmithingTransformRecipe {
    pub properties: RecipeProperties,
    pub template: Option<Ingredient>,
    pub base: Ingredient,
    pub addition: Option<Ingredient>,
    pub result: ItemStackTemplate,
}

#[derive(Debug)]
pub struct SmithingTrimRecipe {
    pub properties: RecipeProperties,
    pub template: Ingredient,
    pub base: Ingredient,
    pub addition: Ingredient,
    pub pattern: TrimPatternRef,
}

/// Every vanilla serializer whose operational type is `minecraft:smithing`.
#[derive(Debug)]
pub enum SmithingRecipe {
    Transform(SmithingTransformRecipe),
    Trim(SmithingTrimRecipe),
}

impl SmithingRecipe {
    #[must_use]
    pub const fn properties(&self) -> &RecipeProperties {
        match self {
            Self::Transform(recipe) => &recipe.properties,
            Self::Trim(recipe) => &recipe.properties,
        }
    }
}

// SAFETY: This Steel-owned key uniquely identifies the unified vanilla smithing data enum.
unsafe impl DowncastType for SmithingRecipe {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:recipe_data/smithing");
}

impl RecipeData for SmithingRecipe {
    fn properties(&self) -> Option<&RecipeProperties> {
        Some(self.properties())
    }
}

#[derive(Debug, Clone)]
pub struct SmithingRecipeInput {
    pub template: ItemStack,
    pub base: ItemStack,
    pub addition: ItemStack,
}

impl SmithingRecipeInput {
    #[must_use]
    pub const fn new(template: ItemStack, base: ItemStack, addition: ItemStack) -> Self {
        Self {
            template,
            base,
            addition,
        }
    }
}

// SAFETY: This Steel-owned key uniquely identifies a smithing matching snapshot.
unsafe impl DowncastType for SmithingRecipeInput {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:recipe_input/smithing");
}

impl RecipeInput for SmithingRecipeInput {
    fn is_empty(&self) -> bool {
        self.template.is_empty() && self.base.is_empty() && self.addition.is_empty()
    }
}

impl RecipeMatches<SmithingRecipeInput> for SmithingRecipe {
    fn matches(&self, input: &SmithingRecipeInput) -> bool {
        let (template, base, addition) = match self {
            Self::Transform(recipe) => (
                recipe.template.as_ref(),
                &recipe.base,
                recipe.addition.as_ref(),
            ),
            Self::Trim(recipe) => (Some(&recipe.template), &recipe.base, Some(&recipe.addition)),
        };
        optional_ingredient_matches(template, &input.template)
            && base.test(&input.base)
            && optional_ingredient_matches(addition, &input.addition)
    }
}

fn optional_ingredient_matches(ingredient: Option<&Ingredient>, stack: &ItemStack) -> bool {
    match ingredient {
        Some(ingredient) => ingredient.test(stack),
        None => stack.is_empty(),
    }
}
