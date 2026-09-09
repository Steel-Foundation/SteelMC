//! Passive recipe metadata and recipe-book categories.

use rustc_hash::FxHashMap;
use steel_utils::Identifier;

use super::Ingredient;

/// Recipe-book grouping used by crafting recipes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CraftingBookCategory {
    Building,
    Redstone,
    Equipment,
    Misc,
}

/// Recipe-book grouping used by cooking recipes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookingBookCategory {
    Food,
    Blocks,
    Misc,
}

/// Logical recipe-book category stored by recipe data.
///
/// Cooking categories are resolved against the operational recipe type when the
/// registry freezes. For example, `Cooking(Food)` becomes `furnace_food` for a
/// smelting recipe and `smoker_food` for a smoking recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeBookCategoryKind {
    Crafting(CraftingBookCategory),
    Cooking(CookingBookCategory),
    Stonecutter,
    Smithing,
}

/// Recipe-book metadata which is not part of matching or processing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeBookProperties {
    pub category: RecipeBookCategoryKind,
    pub group: Option<String>,
    pub show_notification: bool,
}

/// Precomputed ingredient placement information.
#[derive(Debug, Clone)]
pub struct PlacementInfo {
    pub ingredients: Vec<Ingredient>,
    /// Maps recipe slots to `ingredients`; `None` represents an empty slot.
    pub slots_to_ingredient: Vec<Option<usize>>,
}

impl PlacementInfo {
    #[must_use]
    pub const fn not_placeable() -> Self {
        Self {
            ingredients: Vec::new(),
            slots_to_ingredient: Vec::new(),
        }
    }

    #[must_use]
    pub fn from_ingredient(ingredient: Ingredient) -> Self {
        if ingredient.is_empty() {
            return Self::not_placeable();
        }
        Self {
            ingredients: vec![ingredient],
            slots_to_ingredient: vec![Some(0)],
        }
    }

    #[must_use]
    pub fn from_optional_ingredients(ingredients: &[Ingredient]) -> Self {
        let mut present = Vec::with_capacity(ingredients.len());
        let mut slots = Vec::with_capacity(ingredients.len());
        for ingredient in ingredients {
            if ingredient.is_empty() {
                slots.push(None);
            } else {
                slots.push(Some(present.len()));
                present.push(ingredient.clone());
            }
        }
        Self {
            ingredients: present,
            slots_to_ingredient: slots,
        }
    }

    #[must_use]
    pub fn from_ingredients(ingredients: &[Ingredient]) -> Self {
        if ingredients.iter().any(Ingredient::is_empty) {
            return Self::not_placeable();
        }
        Self {
            ingredients: ingredients.to_vec(),
            slots_to_ingredient: (0..ingredients.len()).map(Some).collect(),
        }
    }

    #[must_use]
    pub const fn is_impossible_to_place(&self) -> bool {
        self.slots_to_ingredient.is_empty()
    }
}

/// Optional metadata common to recipe data implementations.
#[derive(Debug, Clone)]
pub struct RecipeProperties {
    pub special: bool,
    pub placement: Option<PlacementInfo>,
    pub recipe_book: Option<RecipeBookProperties>,
}

impl RecipeProperties {
    #[must_use]
    pub const fn special() -> Self {
        Self {
            special: true,
            placement: None,
            recipe_book: None,
        }
    }
}

/// A registered protocol recipe-book category.
#[derive(Debug)]
pub struct RecipeBookCategory {
    pub key: Identifier,
}

pub type RecipeBookCategoryRef = &'static RecipeBookCategory;

/// Registry for protocol-visible recipe-book categories.
pub struct RecipeBookCategoryRegistry {
    categories: Vec<RecipeBookCategoryRef>,
    by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl RecipeBookCategoryRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            categories: Vec::new(),
            by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    RecipeBookCategoryRegistry,
    RecipeBookCategoryRef,
    categories,
    by_key,
    allows_registering,
    "Cannot register duplicate recipe book category key: {}"
);
crate::impl_registry!(
    RecipeBookCategoryRegistry,
    RecipeBookCategory,
    categories,
    by_key,
    recipe_book_categories
);

/// Vanilla protocol recipe-book categories.
pub mod vanilla_recipe_book_categories {
    use super::{RecipeBookCategory, RecipeBookCategoryRegistry};
    use steel_utils::Identifier;

    macro_rules! category {
        ($name:ident, $key:literal) => {
            pub static $name: RecipeBookCategory = RecipeBookCategory {
                key: Identifier::vanilla_static($key),
            };
        };
    }

    category!(CRAFTING_BUILDING_BLOCKS, "crafting_building_blocks");
    category!(CRAFTING_REDSTONE, "crafting_redstone");
    category!(CRAFTING_EQUIPMENT, "crafting_equipment");
    category!(CRAFTING_MISC, "crafting_misc");
    category!(FURNACE_FOOD, "furnace_food");
    category!(FURNACE_BLOCKS, "furnace_blocks");
    category!(FURNACE_MISC, "furnace_misc");
    category!(BLAST_FURNACE_BLOCKS, "blast_furnace_blocks");
    category!(BLAST_FURNACE_MISC, "blast_furnace_misc");
    category!(SMOKER_FOOD, "smoker_food");
    category!(STONECUTTER, "stonecutter");
    category!(SMITHING, "smithing");
    category!(CAMPFIRE, "campfire");

    pub(crate) fn register(registry: &mut RecipeBookCategoryRegistry) {
        registry.register(&CRAFTING_BUILDING_BLOCKS);
        registry.register(&CRAFTING_REDSTONE);
        registry.register(&CRAFTING_EQUIPMENT);
        registry.register(&CRAFTING_MISC);
        registry.register(&FURNACE_FOOD);
        registry.register(&FURNACE_BLOCKS);
        registry.register(&FURNACE_MISC);
        registry.register(&BLAST_FURNACE_BLOCKS);
        registry.register(&BLAST_FURNACE_MISC);
        registry.register(&SMOKER_FOOD);
        registry.register(&STONECUTTER);
        registry.register(&SMITHING);
        registry.register(&CAMPFIRE);
    }
}
