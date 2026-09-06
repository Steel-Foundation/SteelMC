//! Typed, plugin-extensible recipe registries and vanilla recipe data.

mod crafting;
mod ingredient;
mod properties;
mod registry;
mod single_item;
mod smithing;
mod types;

pub use crafting::{
    BannerDuplicateRecipe, BookCloningRecipe, CraftingInput, CraftingRecipe, DecoratedPotRecipe,
    DyeRecipe, FireworkRocketRecipe, FireworkStarFadeRecipe, FireworkStarRecipe, ImbueRecipe,
    MapExtendingRecipe, MapRecipeData, PositionedCraftingInput, RepairItemRecipe, ShapedRecipe,
    ShapelessRecipe, ShieldDecorationRecipe, TransmuteRecipe,
};
pub use ingredient::Ingredient;
pub use properties::{
    CookingBookCategory, CraftingBookCategory, PlacementInfo, RecipeBookCategory,
    RecipeBookCategoryKind, RecipeBookCategoryRef, RecipeBookCategoryRegistry,
    RecipeBookProperties, RecipeProperties, vanilla_recipe_book_categories,
};
pub use registry::{
    CachedRecipeCheck, RecipeRegistry, TypedRecipeRef, TypedRecipeSet, UntypedRecipeRef,
};
pub use single_item::{CookingRecipe, SingleItemRecipeInput, StonecuttingRecipe};
pub use smithing::{
    SmithingRecipe, SmithingRecipeInput, SmithingTransformRecipe, SmithingTrimRecipe,
};
pub use types::{
    Recipe, RecipeData, RecipeInput, RecipeMatches, RecipeType, RecipeTypeEntry,
    RecipeTypeEntryRef, RecipeTypeRegistry, vanilla_recipe_types,
};

pub(crate) use types::ErasedRecipe;
