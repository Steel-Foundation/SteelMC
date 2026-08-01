//! Recipe catalog keyed by vanilla recipe identifiers.

use rustc_hash::FxHashMap;
use steel_utils::Identifier;

use crate::item_stack::ItemStack;

use super::{
    CookingRecipe, CookingRecipeKind, CraftingInput, CraftingRecipe, Recipe, RecipeHolder,
    RecipeHolderRef, ShapedRecipe, ShapelessRecipe, SmeltingRecipe,
};

/// All registered recipes and their type-specific lookup indexes.
pub struct RecipeRegistry {
    recipes: Vec<RecipeHolderRef>,
    recipes_by_key: FxHashMap<Identifier, RecipeHolderRef>,
    crafting_recipes: Vec<CraftingRecipe>,
    smelting_recipes: Vec<CookingRecipe>,
    shaped_recipes: Vec<&'static ShapedRecipe>,
    shapeless_recipes: Vec<&'static ShapelessRecipe>,
    allows_registering: bool,
}

impl Default for RecipeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl RecipeRegistry {
    /// Creates an empty recipe catalog.
    #[must_use]
    pub fn new() -> Self {
        Self {
            recipes: Vec::new(),
            recipes_by_key: FxHashMap::default(),
            crafting_recipes: Vec::new(),
            smelting_recipes: Vec::new(),
            shaped_recipes: Vec::new(),
            shapeless_recipes: Vec::new(),
            allows_registering: true,
        }
    }

    /// Registers one recipe under its stable key.
    pub fn register(&mut self, key: Identifier, value: &'static dyn Recipe) -> RecipeHolderRef {
        assert!(
            self.allows_registering,
            "Cannot register recipes after the registry has been frozen"
        );
        assert!(
            !self.recipes_by_key.contains_key(&key),
            "Cannot register duplicate recipe key: {key}"
        );

        let holder = Box::leak(Box::new(RecipeHolder::new(key.clone(), value)));
        self.recipes_by_key.insert(key, holder);
        self.recipes.push(holder);
        holder
    }

    /// Finalizes deterministic recipe ordering and type-specific indexes.
    pub fn freeze(&mut self) {
        self.recipes.sort_unstable_by(|left, right| left.key().cmp(right.key()));
        self.crafting_recipes.clear();
        self.smelting_recipes.clear();
        self.shaped_recipes.clear();
        self.shapeless_recipes.clear();

        for holder in self.recipes.iter().copied() {
            if let Some(recipe) = holder.crafting() {
                self.crafting_recipes.push(recipe);
            }
            if let Some(recipe) = holder.cooking()
                && recipe.value().kind() == CookingRecipeKind::Smelting
            {
                self.smelting_recipes.push(recipe);
            }
            if let Some(recipe) = holder.downcast_ref::<ShapedRecipe>() {
                self.shaped_recipes.push(recipe);
            }
            if let Some(recipe) = holder.downcast_ref::<ShapelessRecipe>() {
                self.shapeless_recipes.push(recipe);
            }
        }

        self.allows_registering = false;
    }

    /// Gets a recipe holder by its stable key.
    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<RecipeHolderRef> {
        self.recipes_by_key.get(key).copied()
    }

    /// Iterates recipes in vanilla identifier order after finalization.
    pub fn iter(&self) -> impl Iterator<Item = RecipeHolderRef> + '_ {
        self.recipes.iter().copied()
    }

    /// Returns the number of registered recipes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.recipes.len()
    }

    /// Returns whether no recipes have been registered.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.recipes.is_empty()
    }

    /// Finds a matching crafting recipe for a positioned input.
    #[must_use]
    pub fn find_crafting_recipe(&self, input: &CraftingInput) -> Option<CraftingRecipe> {
        self.crafting_recipes
            .iter()
            .copied()
            .find(|recipe| recipe.value().matches(input))
    }

    /// Finds a matching crafting recipe that fits a player 2x2 grid.
    #[must_use]
    pub fn find_crafting_recipe_2x2(&self, input: &CraftingInput) -> Option<CraftingRecipe> {
        self.crafting_recipes
            .iter()
            .copied()
            .find(|recipe| recipe.value().fits_in_2x2() && recipe.value().matches(input))
    }

    /// Gets a shaped recipe by its recipe key.
    #[must_use]
    pub fn get_shaped(&self, key: &Identifier) -> Option<&'static ShapedRecipe> {
        self.by_key(key)?.downcast_ref::<ShapedRecipe>()
    }

    /// Gets a shapeless recipe by its recipe key.
    #[must_use]
    pub fn get_shapeless(&self, key: &Identifier) -> Option<&'static ShapelessRecipe> {
        self.by_key(key)?.downcast_ref::<ShapelessRecipe>()
    }

    /// Finds the first furnace smelting result for `input`.
    #[must_use]
    pub fn find_smelting_result(
        &self,
        input: &ItemStack,
        use_input_count: bool,
    ) -> Option<ItemStack> {
        self.smelting_recipes
            .iter()
            .copied()
            .find(|recipe| recipe.value().matches(input))
            .map(|recipe| recipe.value().assemble_result(input.count(), use_input_count))
    }

    /// Returns the number of shaped crafting recipes.
    #[must_use]
    pub const fn shaped_count(&self) -> usize {
        self.shaped_recipes.len()
    }

    /// Returns the number of shapeless crafting recipes.
    #[must_use]
    pub const fn shapeless_count(&self) -> usize {
        self.shapeless_recipes.len()
    }

    /// Returns the number of furnace smelting recipes.
    #[must_use]
    pub const fn smelting_count(&self) -> usize {
        self.smelting_recipes.len()
    }

    /// Iterates shaped recipes in vanilla identifier order.
    pub fn iter_shaped(&self) -> impl Iterator<Item = &'static ShapedRecipe> + '_ {
        self.shaped_recipes.iter().copied()
    }

    /// Iterates shapeless recipes in vanilla identifier order.
    pub fn iter_shapeless(&self) -> impl Iterator<Item = &'static ShapelessRecipe> + '_ {
        self.shapeless_recipes.iter().copied()
    }

    /// Iterates furnace smelting recipes in vanilla identifier order.
    pub fn iter_smelting(&self) -> impl Iterator<Item = &'static SmeltingRecipe> + '_ {
        self.smelting_recipes
            .iter()
            .filter_map(|recipe| recipe.holder().downcast_ref::<SmeltingRecipe>())
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::{DowncastType, DowncastTypeKey, Identifier};

    use crate::{item_stack::ItemStack, test_support::init_test_registry, vanilla_items};

    use super::*;
    use crate::recipe::{CraftingCategory, Ingredient, RecipeResult};

    #[derive(Debug)]
    struct TestRecipe;

    // SAFETY: This test-only key uniquely identifies the concrete test recipe.
    unsafe impl DowncastType for TestRecipe {
        const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:test/recipe");
    }

    impl Recipe for TestRecipe {}

    static TEST_RECIPE: TestRecipe = TestRecipe;

    #[test]
    fn finalization_orders_recipe_holders_by_vanilla_identifier() {
        let mut first = RecipeRegistry::new();
        first.register(Identifier::new_static("z", "apple"), &TEST_RECIPE);
        first.register(Identifier::new_static("a", "banana"), &TEST_RECIPE);
        first.register(Identifier::new_static("z", "banana"), &TEST_RECIPE);
        first.freeze();

        let mut second = RecipeRegistry::new();
        second.register(Identifier::new_static("z", "banana"), &TEST_RECIPE);
        second.register(Identifier::new_static("a", "banana"), &TEST_RECIPE);
        second.register(Identifier::new_static("z", "apple"), &TEST_RECIPE);
        second.freeze();

        let keys: Vec<_> = first.iter().map(|recipe| recipe.key().to_string()).collect();
        assert_eq!(keys, ["z:apple", "a:banana", "z:banana"]);
        assert_eq!(
            keys,
            second
                .iter()
                .map(|recipe| recipe.key().to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            first.by_key(&Identifier::new_static("a", "banana")),
            first.iter().nth(1)
        );
    }

    #[test]
    #[should_panic(expected = "Cannot register duplicate recipe key")]
    fn duplicate_recipe_keys_are_rejected() {
        let mut registry = RecipeRegistry::new();
        let key = Identifier::vanilla_static("duplicate");
        registry.register(key.clone(), &TEST_RECIPE);
        registry.register(key, &TEST_RECIPE);
    }

    #[test]
    #[should_panic(expected = "Cannot register recipes after the registry has been frozen")]
    fn registration_after_freeze_is_rejected() {
        let mut registry = RecipeRegistry::new();
        registry.freeze();
        registry.register(Identifier::vanilla_static("late"), &TEST_RECIPE);
    }

    #[test]
    fn crafting_matching_follows_recipe_key_order_across_recipe_shapes() {
        init_test_registry();
        let ingredient: &'static [Ingredient] =
            Box::leak(Box::new([Ingredient::Item(&vanilla_items::STICK)]));
        let shaped = Box::leak(Box::new(ShapedRecipe::new(
            "shaped",
            CraftingCategory::Misc,
            1,
            1,
            ingredient,
            RecipeResult {
                item: &vanilla_items::PAPER,
                count: 1,
            },
            true,
        )));
        let shapeless = Box::leak(Box::new(ShapelessRecipe {
            group: "shapeless",
            category: CraftingCategory::Misc,
            ingredients: ingredient,
            result: RecipeResult {
                item: &vanilla_items::BOOK,
                count: 1,
            },
        }));
        let mut registry = RecipeRegistry::new();
        registry.register(Identifier::vanilla_static("z_shaped"), shaped);
        registry.register(Identifier::vanilla_static("a_shapeless"), shapeless);
        registry.freeze();

        let input = CraftingInput::new(
            1,
            1,
            vec![ItemStack::with_count(&vanilla_items::STICK, 1)],
        );
        let recipe = registry
            .find_crafting_recipe(&input)
            .unwrap_or_else(|| panic!("matching recipe should be found"));

        assert_eq!(recipe.key(), &Identifier::vanilla_static("a_shapeless"));
        assert_eq!(recipe.holder().value().group(), "shapeless");
        assert!(recipe.holder().value().show_notification());
        assert!(recipe.holder().downcast_ref::<ShapelessRecipe>().is_some());
    }

    #[test]
    fn generated_catalog_includes_every_supported_recipe_type() {
        init_test_registry();
        let registry = &crate::REGISTRY.recipes;

        assert_eq!(
            registry.len(),
            registry.shaped_count() + registry.shapeless_count() + registry.smelting_count()
        );
        assert!(registry.smelting_count() > 0);
        let Some(smelting) = registry.iter_smelting().next() else {
            panic!("a smelting recipe should be generated");
        };
        assert!(smelting.holder().downcast_ref::<SmeltingRecipe>().is_some());
        let Some(crafting_table) = registry.by_key(&Identifier::vanilla_static("crafting_table"))
        else {
            panic!("crafting table recipe should be generated");
        };
        assert!(!crafting_table.value().show_notification());
        assert!(crafting_table.downcast_ref::<ShapedRecipe>().is_some());
    }
}
