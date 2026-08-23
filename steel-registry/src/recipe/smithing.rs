//! Smithing recipes.

use steel_utils::Identifier;

use crate::data_components::components::ArmorTrim;
use crate::data_components::vanilla_components::{PROVIDES_TRIM_MATERIAL, TRIM};
use crate::item_stack::ItemStack;
use crate::registry::RegistryHolder;
use crate::{REGISTRY, RegistryExt};

use super::{Ingredient, RecipeResult};

/// Vanilla `SmithingTransformRecipe`.
#[derive(Debug)]
pub struct SmithingTransformRecipe {
    /// Recipe identifier.
    pub id: Identifier,
    /// Optional template ingredient.
    pub template: Option<Ingredient>,
    /// Base item ingredient.
    pub base: Ingredient,
    /// Optional addition ingredient.
    pub addition: Option<Ingredient>,
    /// Result item.
    pub result: RecipeResult,
}

/// Vanilla `SmithingTrimRecipe`.
#[derive(Debug)]
pub struct SmithingTrimRecipe {
    /// Recipe identifier.
    pub id: Identifier,
    /// Template ingredient.
    pub template: Ingredient,
    /// Base item ingredient.
    pub base: Ingredient,
    /// Addition ingredient.
    pub addition: Ingredient,
    /// Trim pattern applied to the base item.
    pub pattern: Identifier,
}

/// A smithing recipe of either vanilla kind.
#[derive(Debug, Clone, Copy)]
pub enum SmithingRecipe {
    /// Netherite-style item upgrade.
    Transform(&'static SmithingTransformRecipe),
    /// Armor trim.
    Trim(&'static SmithingTrimRecipe),
}

fn optional_ingredient_matches(ingredient: Option<&Ingredient>, stack: &ItemStack) -> bool {
    match ingredient {
        Some(ingredient) => ingredient.test(stack),
        None => stack.is_empty(),
    }
}

impl SmithingTransformRecipe {
    /// Returns whether this recipe accepts the smithing input.
    #[must_use]
    pub fn matches(&self, template: &ItemStack, base: &ItemStack, addition: &ItemStack) -> bool {
        optional_ingredient_matches(self.template.as_ref(), template)
            && self.base.test(base)
            && optional_ingredient_matches(self.addition.as_ref(), addition)
    }

    /// Assembles the upgraded stack, keeping the base item's components.
    #[must_use]
    pub fn assemble(&self, base: &ItemStack) -> ItemStack {
        let mut stack = base.copy_with_count(self.result.count);
        stack.set_item(&self.result.item.key);
        stack
    }
}

impl SmithingTrimRecipe {
    /// Returns whether this recipe accepts the smithing input.
    #[must_use]
    pub fn matches(&self, template: &ItemStack, base: &ItemStack, addition: &ItemStack) -> bool {
        self.template.test(template) && self.base.test(base) && self.addition.test(addition)
    }

    /// Applies this recipe's trim pattern to `base` using `addition`'s material.
    #[must_use]
    pub fn assemble(&self, base: &ItemStack, addition: &ItemStack) -> ItemStack {
        apply_trim(base, addition, &self.pattern)
    }
}

/// Vanilla `SmithingTrimRecipe.applyTrim`.
#[must_use]
pub fn apply_trim(base: &ItemStack, addition: &ItemStack, pattern_id: &Identifier) -> ItemStack {
    let Some(provided) = addition.get(PROVIDES_TRIM_MATERIAL) else {
        return ItemStack::empty();
    };
    let Some(pattern) = REGISTRY.trim_patterns.by_key(pattern_id) else {
        return ItemStack::empty();
    };
    let new_trim = ArmorTrim::new(
        provided.material().clone(),
        RegistryHolder::reference(pattern),
    );
    if base.get(TRIM) == Some(&new_trim) {
        return ItemStack::empty();
    }
    let mut trimmed = base.copy_with_count(1);
    trimmed.set(TRIM, new_trim);
    trimmed
}

impl SmithingRecipe {
    /// Returns whether this recipe accepts the smithing input.
    #[must_use]
    pub fn matches(&self, template: &ItemStack, base: &ItemStack, addition: &ItemStack) -> bool {
        match self {
            Self::Transform(recipe) => recipe.matches(template, base, addition),
            Self::Trim(recipe) => recipe.matches(template, base, addition),
        }
    }

    /// Assembles the smithing result.
    #[must_use]
    pub fn assemble(
        &self,
        template: &ItemStack,
        base: &ItemStack,
        addition: &ItemStack,
    ) -> ItemStack {
        let _ = template;
        match self {
            Self::Transform(recipe) => recipe.assemble(base),
            Self::Trim(recipe) => recipe.assemble(base, addition),
        }
    }

    /// Template ingredient, if the recipe uses one.
    #[must_use]
    pub const fn template_ingredient(self) -> Option<&'static Ingredient> {
        match self {
            Self::Transform(recipe) => recipe.template.as_ref(),
            Self::Trim(recipe) => Some(&recipe.template),
        }
    }

    /// Base item ingredient.
    #[must_use]
    pub const fn base_ingredient(self) -> &'static Ingredient {
        match self {
            Self::Transform(recipe) => &recipe.base,
            Self::Trim(recipe) => &recipe.base,
        }
    }

    /// Addition ingredient, if the recipe uses one.
    #[must_use]
    pub const fn addition_ingredient(self) -> Option<&'static Ingredient> {
        match self {
            Self::Transform(recipe) => recipe.addition.as_ref(),
            Self::Trim(recipe) => Some(&recipe.addition),
        }
    }
}
