//! Typed recipe discriminators and recipe entries.

use std::fmt::{self, Debug, Formatter};
use std::marker::PhantomData;

use rustc_hash::FxHashMap;
use steel_utils::{DowncastType, DowncastTypeKey, ErasedType, Identifier};

use super::RecipeProperties;

/// Passive data stored by a recipe.
pub trait RecipeData: ErasedType + Debug + Send + Sync + 'static {
    fn properties(&self) -> Option<&RecipeProperties> {
        None
    }
}

/// Immutable input snapshot used while matching a recipe type.
pub trait RecipeInput: DowncastType + Debug + Send + Sync + 'static {
    #[must_use]
    fn is_empty(&self) -> bool;
}

/// Matching behavior implemented by one recipe data type for its input snapshot.
pub trait RecipeMatches<I: RecipeInput>: RecipeData {
    /// Returns whether this recipe accepts the provided input.
    #[must_use]
    fn matches(&self, input: &I) -> bool;
}

/// Type-erased registered recipe discriminator.
#[derive(Debug, PartialEq, Eq)]
pub struct RecipeTypeEntry {
    pub key: Identifier,
    data_type_key: DowncastTypeKey,
    input_type_key: DowncastTypeKey,
}

impl RecipeTypeEntry {
    #[must_use]
    pub const fn data_type_key(&self) -> DowncastTypeKey {
        self.data_type_key
    }

    #[must_use]
    pub const fn input_type_key(&self) -> DowncastTypeKey {
        self.input_type_key
    }
}

pub type RecipeTypeEntryRef = &'static RecipeTypeEntry;

/// Typed handle for an operational recipe type.
pub struct RecipeType<D: RecipeMatches<I> + DowncastType, I: RecipeInput> {
    entry: RecipeTypeEntry,
    _marker: PhantomData<fn(&D, &I)>,
}

impl<D: RecipeMatches<I> + DowncastType, I: RecipeInput> RecipeType<D, I> {
    #[must_use]
    pub const fn new(key: Identifier) -> Self {
        Self {
            entry: RecipeTypeEntry {
                key,
                data_type_key: D::TYPE_KEY,
                input_type_key: I::TYPE_KEY,
            },
            _marker: PhantomData,
        }
    }

    #[must_use]
    pub const fn entry(&'static self) -> RecipeTypeEntryRef {
        &self.entry
    }

    #[must_use]
    pub const fn key(&self) -> &Identifier {
        &self.entry.key
    }
}

impl<D: RecipeMatches<I> + DowncastType, I: RecipeInput> Debug for RecipeType<D, I> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RecipeType")
            .field("key", &self.entry.key)
            .field("data_type_key", &self.entry.data_type_key)
            .field("input_type_key", &self.entry.input_type_key)
            .finish_non_exhaustive()
    }
}

/// A keyed recipe with concrete passive data and input types.
pub struct Recipe<D: RecipeMatches<I> + DowncastType, I: RecipeInput> {
    key: Identifier,
    recipe_type: &'static RecipeType<D, I>,
    data: D,
}

impl<D: RecipeMatches<I> + DowncastType, I: RecipeInput> Recipe<D, I> {
    #[must_use]
    pub const fn new(key: Identifier, recipe_type: &'static RecipeType<D, I>, data: D) -> Self {
        Self {
            key,
            recipe_type,
            data,
        }
    }

    #[must_use]
    pub const fn key(&self) -> &Identifier {
        &self.key
    }

    #[must_use]
    pub const fn recipe_type(&self) -> &'static RecipeType<D, I> {
        self.recipe_type
    }

    #[must_use]
    pub const fn data(&self) -> &D {
        &self.data
    }
}

impl<D: RecipeMatches<I> + DowncastType, I: RecipeInput> Debug for Recipe<D, I> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Recipe")
            .field("key", &self.key)
            .field("recipe_type", &self.recipe_type.key())
            .field("data", &self.data)
            .finish()
    }
}

pub(crate) trait ErasedRecipe: Debug + Send + Sync {
    fn key(&self) -> &Identifier;
    fn recipe_type(&self) -> RecipeTypeEntryRef;
    fn data(&self) -> &dyn RecipeData;
}

impl<D: RecipeMatches<I> + DowncastType, I: RecipeInput> ErasedRecipe for Recipe<D, I> {
    fn key(&self) -> &Identifier {
        self.key()
    }

    fn recipe_type(&self) -> RecipeTypeEntryRef {
        self.recipe_type.entry()
    }

    fn data(&self) -> &dyn RecipeData {
        &self.data
    }
}

/// Registry for operational recipe types.
pub struct RecipeTypeRegistry {
    types: Vec<RecipeTypeEntryRef>,
    by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl RecipeTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            types: Vec::new(),
            by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register<D: RecipeMatches<I> + DowncastType, I: RecipeInput>(
        &mut self,
        recipe_type: &'static RecipeType<D, I>,
    ) {
        self.register_entry(recipe_type.entry());
    }

    fn register_entry(&mut self, entry: RecipeTypeEntryRef) {
        assert!(
            self.allows_registering,
            "Cannot register recipe types after the registry has been frozen"
        );
        assert!(
            !self.by_key.contains_key(&entry.key),
            "Cannot register duplicate recipe type key: {}",
            entry.key
        );
        let id = self.types.len();
        self.types.push(entry);
        self.by_key.insert(entry.key.clone(), id);
    }

    pub const fn freeze(&mut self) {
        self.allows_registering = false;
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<RecipeTypeEntryRef> {
        self.by_key
            .get(key)
            .and_then(|id| self.types.get(*id))
            .copied()
    }

    #[must_use]
    pub fn contains<D: RecipeMatches<I> + DowncastType, I: RecipeInput>(
        &self,
        recipe_type: &'static RecipeType<D, I>,
    ) -> bool {
        self.by_key(recipe_type.key())
            .is_some_and(|registered| std::ptr::eq(registered, recipe_type.entry()))
    }

    pub fn iter(&self) -> impl Iterator<Item = RecipeTypeEntryRef> + '_ {
        self.types.iter().copied()
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.types.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.types.is_empty()
    }
}

impl Default for RecipeTypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Vanilla operational recipe types.
pub mod vanilla_recipe_types {
    use steel_utils::Identifier;

    use super::{RecipeType, RecipeTypeRegistry};
    use crate::recipe::{
        CookingRecipe, CraftingInput, CraftingRecipe, SingleItemRecipeInput, SmithingRecipe,
        SmithingRecipeInput, StonecuttingRecipe,
    };

    const fn cooking_type(key: &'static str) -> RecipeType<CookingRecipe, SingleItemRecipeInput> {
        RecipeType::new(Identifier::vanilla_static(key))
    }

    pub static CRAFTING: RecipeType<CraftingRecipe, CraftingInput> =
        RecipeType::new(Identifier::vanilla_static("crafting"));
    pub static SMELTING: RecipeType<CookingRecipe, SingleItemRecipeInput> =
        cooking_type("smelting");
    pub static BLASTING: RecipeType<CookingRecipe, SingleItemRecipeInput> =
        cooking_type("blasting");
    pub static SMOKING: RecipeType<CookingRecipe, SingleItemRecipeInput> = cooking_type("smoking");
    pub static CAMPFIRE_COOKING: RecipeType<CookingRecipe, SingleItemRecipeInput> =
        cooking_type("campfire_cooking");
    pub static STONECUTTING: RecipeType<StonecuttingRecipe, SingleItemRecipeInput> =
        RecipeType::new(Identifier::vanilla_static("stonecutting"));
    pub static SMITHING: RecipeType<SmithingRecipe, SmithingRecipeInput> =
        RecipeType::new(Identifier::vanilla_static("smithing"));

    pub(crate) fn register(registry: &mut RecipeTypeRegistry) {
        registry.register(&CRAFTING);
        registry.register(&SMELTING);
        registry.register(&BLASTING);
        registry.register(&SMOKING);
        registry.register(&CAMPFIRE_COOKING);
        registry.register(&STONECUTTING);
        registry.register(&SMITHING);
    }
}
