//! Recipe identity and behavior capabilities.

use std::fmt::{self, Debug, Formatter};

use steel_utils::{Downcast as _, DowncastType, ErasedType, Identifier};

use crate::item_stack::ItemStack;

use super::{CraftingCategory, CraftingInput, RecipeResult};

/// Shared behavior for a registered recipe implementation.
pub trait Recipe: ErasedType + Debug + Send + Sync {
    /// Vanilla `Recipe.group()`.
    fn group(&self) -> &str {
        ""
    }

    /// Vanilla `Recipe.showNotification()`.
    fn show_notification(&self) -> bool {
        true
    }

    /// Vanilla `Recipe.isSpecial()`.
    fn is_special(&self) -> bool {
        false
    }

    /// Returns the crafting capability when this recipe can use a crafting grid.
    fn as_crafting(&self) -> Option<&dyn CraftingRecipeBehavior> {
        None
    }

    /// Returns the cooking capability when this recipe uses a cooking menu.
    fn as_cooking(&self) -> Option<&dyn CookingRecipeBehavior> {
        None
    }
}

/// Behavior shared by recipes that can be placed in a crafting grid.
pub trait CraftingRecipeBehavior: Debug + Send + Sync {
    /// The recipe-book category selected by this crafting recipe.
    fn category(&self) -> CraftingCategory;

    /// The displayed and crafted result.
    fn result(&self) -> &RecipeResult;

    /// Tests the positioned crafting input.
    fn matches(&self, input: &CraftingInput) -> bool;

    /// Creates the crafting result.
    fn assemble(&self) -> ItemStack;

    /// Returns crafting remainders for the positioned input.
    fn remaining_items(&self, input: &CraftingInput) -> Vec<ItemStack>;

    /// Returns whether this recipe fits in a player 2x2 grid.
    fn fits_in_2x2(&self) -> bool;
}

/// The currently supported vanilla cooking menu kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookingRecipeKind {
    /// A furnace smelting recipe.
    Smelting,
}

/// Behavior shared by recipes that can use a cooking menu.
pub trait CookingRecipeBehavior: Debug + Send + Sync {
    /// The vanilla cooking menu kind this recipe belongs to.
    fn kind(&self) -> CookingRecipeKind;

    /// Tests one input stack.
    fn matches(&self, input: &ItemStack) -> bool;

    /// Creates the result for an input count.
    fn assemble_result(&self, input_count: i32, use_input_count: bool) -> ItemStack;
}

/// A key and its recipe implementation, mirroring vanilla's `RecipeHolder`.
pub struct RecipeHolder {
    key: Identifier,
    value: &'static dyn Recipe,
}

impl RecipeHolder {
    /// Creates a holder for one registered recipe implementation.
    #[must_use]
    pub const fn new(key: Identifier, value: &'static dyn Recipe) -> Self {
        Self { key, value }
    }

    /// Returns the stable recipe key.
    #[must_use]
    pub const fn key(&self) -> &Identifier {
        &self.key
    }

    /// Returns the erased recipe behavior.
    #[must_use]
    pub const fn value(&self) -> &'static dyn Recipe {
        self.value
    }

    /// Returns this holder's crafting capability.
    #[must_use]
    pub fn crafting(&'static self) -> Option<CraftingRecipe> {
        self.value
            .as_crafting()
            .map(|value| TypedRecipe::new(self, value))
    }

    /// Returns this holder's cooking capability.
    #[must_use]
    pub fn cooking(&'static self) -> Option<CookingRecipe> {
        self.value
            .as_cooking()
            .map(|value| TypedRecipe::new(self, value))
    }

    /// Returns the concrete implementation when Steel owns its type.
    #[must_use]
    pub fn downcast_ref<T: DowncastType>(&self) -> Option<&T> {
        self.value.downcast_ref::<T>()
    }
}

impl Debug for RecipeHolder {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RecipeHolder")
            .field("key", &self.key)
            .field("type_key", &self.value.downcast_type_key())
            .finish()
    }
}

impl PartialEq for RecipeHolder {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for RecipeHolder {}

/// A stable reference to a registered recipe holder.
pub type RecipeHolderRef = &'static RecipeHolder;

/// A holder paired with one of its behavior capabilities.
pub struct TypedRecipe<T: ?Sized + 'static> {
    holder: RecipeHolderRef,
    value: &'static T,
}

impl<T: ?Sized + 'static> TypedRecipe<T> {
    fn new(holder: RecipeHolderRef, value: &'static T) -> Self {
        Self { holder, value }
    }

    /// Returns the recipe holder and its stable key.
    #[must_use]
    pub const fn holder(&self) -> RecipeHolderRef {
        self.holder
    }

    /// Returns the stable recipe key.
    #[must_use]
    pub const fn key(&self) -> &Identifier {
        self.holder.key()
    }

    /// Returns the typed recipe behavior.
    #[must_use]
    pub const fn value(&self) -> &'static T {
        self.value
    }
}

impl<T: ?Sized + 'static> Copy for TypedRecipe<T> {}

impl<T: ?Sized + 'static> Clone for TypedRecipe<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: ?Sized + 'static> Debug for TypedRecipe<T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("TypedRecipe").field(&self.key()).finish()
    }
}

impl<T: ?Sized + 'static> PartialEq for TypedRecipe<T> {
    fn eq(&self, other: &Self) -> bool {
        self.holder == other.holder
    }
}

impl<T: ?Sized + 'static> Eq for TypedRecipe<T> {}

/// A recipe holder with crafting behavior.
pub type CraftingRecipe = TypedRecipe<dyn CraftingRecipeBehavior>;

/// A recipe holder with cooking behavior.
pub type CookingRecipe = TypedRecipe<dyn CookingRecipeBehavior>;
