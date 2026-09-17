//! Maps extracted serializer identifiers to recipe-family generators and types.

use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;

use super::RecipeGenerator;
use super::cooking::CookingSerializer;
use super::{crafting, smithing, stonecutting};

#[derive(Clone, Copy)]
pub(super) enum RecipeSerializer {
    Crafting(RecipeGenerator),
    Cooking(CookingSerializer),
    Stonecutting,
    Smithing(RecipeGenerator),
}

impl RecipeSerializer {
    pub(super) fn from_identifier(identifier: &str) -> Option<Self> {
        let path = identifier.strip_prefix("minecraft:")?;
        if let Some(generate) = crafting::generator(path) {
            return Some(Self::Crafting(generate));
        }
        if let Some(serializer) = CookingSerializer::from_path(path) {
            return Some(Self::Cooking(serializer));
        }
        if path == "stonecutting" {
            return Some(Self::Stonecutting);
        }
        smithing::generator(path).map(Self::Smithing)
    }

    pub(super) fn generate_data(self, value: &Value) -> TokenStream {
        match self {
            Self::Crafting(generate) | Self::Smithing(generate) => generate(value),
            Self::Cooking(serializer) => serializer.generate(value),
            Self::Stonecutting => stonecutting::generate(value),
        }
    }

    pub(super) fn recipe_type_tokens(self) -> (TokenStream, TokenStream) {
        match self {
            Self::Crafting(_) => (
                quote! { Recipe<CraftingRecipe, CraftingInput> },
                quote! { vanilla_recipe_types::CRAFTING },
            ),
            Self::Cooking(serializer) => cooking_type_tokens(serializer.recipe_type_tokens()),
            Self::Stonecutting => (
                quote! { Recipe<StonecuttingRecipe, SingleItemRecipeInput> },
                quote! { vanilla_recipe_types::STONECUTTING },
            ),
            Self::Smithing(_) => (
                quote! { Recipe<SmithingRecipe, SmithingRecipeInput> },
                quote! { vanilla_recipe_types::SMITHING },
            ),
        }
    }
}

fn cooking_type_tokens(recipe_type: TokenStream) -> (TokenStream, TokenStream) {
    (
        quote! { Recipe<CookingRecipe, SingleItemRecipeInput> },
        quote! { vanilla_recipe_types::#recipe_type },
    )
}
