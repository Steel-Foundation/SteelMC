//! Generator for the vanilla stonecutting serializer.

use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;

use super::shared::{book_properties, field, ingredient_tokens, result_tokens};

pub(super) fn generate(value: &Value) -> TokenStream {
    let ingredient = ingredient_tokens(field(value, "ingredient"));
    let result = result_tokens(field(value, "result"));
    let properties = book_properties(quote! { RecipeBookCategoryKind::Stonecutter }, value);
    quote! {{
        let ingredient = #ingredient;
        let mut properties = #properties;
        properties.placement = Some(PlacementInfo::from_ingredient(ingredient.clone()));
        StonecuttingRecipe { properties, ingredient, result: #result }
    }}
}
