//! Generator for the vanilla brewing serializer.

use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;

use super::shared::{field, ingredient_tokens, result_tokens, string_field, vanilla_ident};

pub(super) fn generate(value: &Value) -> TokenStream {
    let input = potion_ingredient_tokens(field(value, "input"));
    let reagent = potion_ingredient_tokens(field(value, "reagent"));
    let result = result_tokens(field(value, "output"));
    quote! {
        BrewingRecipe {
            properties: RecipeProperties::special(),
            input: #input,
            reagent: #reagent,
            result: #result,
        }
    }
}

fn potion_ingredient_tokens(value: &Value) -> TokenStream {
    let item = ingredient_tokens(field(value, "item"));
    let potion = if let Some(contents) = value.get("potion_contents") {
        let predicate = contents
            .as_object()
            .unwrap_or_else(|| panic!("Brewing potion predicate is not an object: {contents}"));
        assert!(
            predicate.len() == 1 && predicate.contains_key("potions"),
            "Unsupported extracted brewing potion predicate {predicate:?}"
        );
        let potion = vanilla_ident(string_field(contents, "potions"));
        quote! { Some(&vanilla_potions::#potion) }
    } else {
        quote! { None }
    };
    quote! { PotionIngredient { item: #item, potion: #potion } }
}
