//! Generator for the brewing recipe serializer.

use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;

use super::shared::{field, ingredient_tokens, result_tokens, vanilla_ident};

pub(super) fn generate(value: &Value) -> TokenStream {
    let input = potion_ingredient_tokens(field(value, "input"));
    let reagent = potion_ingredient_tokens(field(value, "reagent"));
    let output = result_tokens(field(value, "output"));
    quote! {{
        BrewingRecipe {
            properties: RecipeProperties::special(),
            input: #input,
            reagent: #reagent,
            output: #output,
        }
    }}
}

fn potion_ingredient_tokens(value: &Value) -> TokenStream {
    let ingredient = ingredient_tokens(field(value, "item"));
    let Some(contents) = value.get("potion_contents") else {
        return quote! { PotionIngredient::new(#ingredient, None) };
    };
    let contents = contents
        .as_object()
        .unwrap_or_else(|| panic!("Brewing potion contents are not an object: {contents}"));
    assert_eq!(
        contents.len(),
        1,
        "Brewing potion contents have unsupported fields {contents:?}"
    );
    let potions = contents
        .get("potions")
        .unwrap_or_else(|| panic!("Brewing potion contents are missing potions: {contents:?}"));
    let potions = potion_holder_set_tokens(potions);
    quote! {
        PotionIngredient::new(
            #ingredient,
            Some(PotionsPredicate::of_potions(#potions)),
        )
    }
}

fn potion_holder_set_tokens(value: &Value) -> TokenStream {
    match value {
        Value::String(potion) => {
            let potion = vanilla_ident(potion);
            quote! { RegistryHolderSet::Direct(vec![&vanilla_potions::#potion]) }
        }
        Value::Array(potions) => {
            let potions: Vec<_> = potions
                .iter()
                .map(|potion| {
                    let potion = potion.as_str().unwrap_or_else(|| {
                        panic!("Brewing potion entry is not a string: {potion}")
                    });
                    let potion = vanilla_ident(potion);
                    quote! { &vanilla_potions::#potion }
                })
                .collect();
            quote! { RegistryHolderSet::Direct(vec![#(#potions),*]) }
        }
        other => panic!("Unsupported brewing potion reference {other}"),
    }
}
