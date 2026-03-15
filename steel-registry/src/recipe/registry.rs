//! Recipe registry for looking up recipes.

use rustc_hash::FxHashMap;
use steel_utils::Identifier;

use super::crafting::{CraftingInput, CraftingRecipe, ShapedRecipe, ShapelessRecipe};

/// Registry for all recipes.
pub struct RecipeRegistry {
    /// All recipes in registration order (unified storage for RegistryExt).
    recipes_by_id: Vec<&'static CraftingRecipe>,
    /// Map from recipe key to index in `recipes_by_id`.
    recipes_by_key: FxHashMap<Identifier, usize>,
    /// All shaped crafting recipes (for type-specific iteration).
    shaped_recipes: Vec<&'static ShapedRecipe>,
    /// All shapeless crafting recipes (for type-specific iteration).
    shapeless_recipes: Vec<&'static ShapelessRecipe>,
    /// Whether registration is still allowed.
    allows_registering: bool,
}

impl Default for RecipeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl RecipeRegistry {
    /// Creates a new empty recipe registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            recipes_by_id: Vec::new(),
            recipes_by_key: FxHashMap::default(),
            shaped_recipes: Vec::new(),
            shapeless_recipes: Vec::new(),
            allows_registering: true,
        }
    }

    /// Registers a shaped recipe.
    pub fn register_shaped(&mut self, recipe: &'static ShapedRecipe) {
        assert!(
            self.allows_registering,
            "Cannot register recipes after the registry has been frozen"
        );
        let id = self.recipes_by_id.len();
        self.recipes_by_key.insert(recipe.id.clone(), id);
        self.recipes_by_id
            .push(Box::leak(Box::new(CraftingRecipe::Shaped(recipe))));
        self.shaped_recipes.push(recipe);
    }

    /// Registers a shapeless recipe.
    pub fn register_shapeless(&mut self, recipe: &'static ShapelessRecipe) {
        assert!(
            self.allows_registering,
            "Cannot register recipes after the registry has been frozen"
        );
        let id = self.recipes_by_id.len();
        self.recipes_by_key.insert(recipe.id.clone(), id);
        self.recipes_by_id
            .push(Box::leak(Box::new(CraftingRecipe::Shapeless(recipe))));
        self.shapeless_recipes.push(recipe);
    }

    /// Finds a matching crafting recipe for the given positioned input.
    /// Returns the first matching recipe, or None if no recipe matches.
    #[must_use]
    pub fn find_crafting_recipe(&self, input: &CraftingInput) -> Option<CraftingRecipe> {
        // Try shaped recipes first (they're more specific)
        for recipe in &self.shaped_recipes {
            if recipe.matches(input) {
                return Some(CraftingRecipe::Shaped(recipe));
            }
        }

        // Then try shapeless
        for recipe in &self.shapeless_recipes {
            if recipe.matches(input) {
                return Some(CraftingRecipe::Shapeless(recipe));
            }
        }

        None
    }

    /// Finds a matching crafting recipe for a 2x2 grid.
    /// Only checks recipes that can fit in a 2x2 grid.
    #[must_use]
    pub fn find_crafting_recipe_2x2(&self, input: &CraftingInput) -> Option<CraftingRecipe> {
        // Try shaped recipes first (they're more specific)
        for recipe in &self.shaped_recipes {
            if recipe.fits_in_2x2() && recipe.matches(input) {
                return Some(CraftingRecipe::Shaped(recipe));
            }
        }

        // Then try shapeless
        for recipe in &self.shapeless_recipes {
            if recipe.fits_in_2x2() && recipe.matches(input) {
                return Some(CraftingRecipe::Shapeless(recipe));
            }
        }

        None
    }

    /// Gets a shaped recipe by its identifier.
    #[must_use]
    pub fn get_shaped(&self, id: &Identifier) -> Option<&'static ShapedRecipe> {
        self.shaped_recipes.iter().find(|r| &r.id == id).copied()
    }

    /// Gets a shapeless recipe by its identifier.
    #[must_use]
    pub fn get_shapeless(&self, id: &Identifier) -> Option<&'static ShapelessRecipe> {
        self.shapeless_recipes.iter().find(|r| &r.id == id).copied()
    }

    /// Returns the number of shaped recipes.
    #[must_use]
    pub fn shaped_count(&self) -> usize {
        self.shaped_recipes.len()
    }

    /// Returns the number of shapeless recipes.
    #[must_use]
    pub fn shapeless_count(&self) -> usize {
        self.shapeless_recipes.len()
    }

    /// Iterates over all shaped recipes.
    pub fn iter_shaped(&self) -> impl Iterator<Item = &'static ShapedRecipe> + '_ {
        self.shaped_recipes.iter().copied()
    }

    /// Iterates over all shapeless recipes.
    pub fn iter_shapeless(&self) -> impl Iterator<Item = &'static ShapelessRecipe> + '_ {
        self.shapeless_recipes.iter().copied()
    }
}

impl crate::RegistryExt for RecipeRegistry {
    type Entry = CraftingRecipe;

    fn freeze(&mut self) {
        self.allows_registering = false;
    }

    fn by_id(&self, id: usize) -> Option<&'static CraftingRecipe> {
        self.recipes_by_id.get(id).copied()
    }

    fn by_key(&self, key: &Identifier) -> Option<&'static CraftingRecipe> {
        self.recipes_by_key
            .get(key)
            .and_then(|&id| self.recipes_by_id.get(id).copied())
    }

    fn id_from_key(&self, key: &Identifier) -> Option<usize> {
        self.recipes_by_key.get(key).copied()
    }

    fn len(&self) -> usize {
        self.recipes_by_id.len()
    }

    fn is_empty(&self) -> bool {
        self.recipes_by_id.is_empty()
    }
}

impl crate::RegistryEntry for CraftingRecipe {
    fn key(&self) -> &Identifier {
        self.id()
    }

    fn try_id(&self) -> Option<usize> {
        use crate::RegistryExt;
        crate::REGISTRY.recipes.id_from_key(self.id())
    }
}
