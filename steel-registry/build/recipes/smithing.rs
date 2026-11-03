//! Generators for vanilla smithing recipe serializers.

use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;

use super::RecipeGenerator;
use super::shared::{
    book_properties, field, ingredient_tokens, optional_ingredient_tokens, result_tokens,
    string_field, vanilla_ident,
};

pub(super) fn generator(path: &str) -> Option<RecipeGenerator> {
    Some(match path {
        "smithing_transform" => generate_transform,
        "smithing_trim" => generate_trim,
        _ => return None,
    })
}

fn generate_transform(value: &Value) -> TokenStream {
    let template = optional_ingredient_tokens(value.get("template"));
    let base = ingredient_tokens(field(value, "base"));
    let addition = optional_ingredient_tokens(value.get("addition"));
    let result = result_tokens(field(value, "result"));
    let properties = book_properties(quote! { RecipeBookCategoryKind::Smithing }, value);
    quote! {{
        let template = #template;
        let base = #base;
        let addition = #addition;
        let mut properties = #properties;
        properties.placement = Some(PlacementInfo::from_optional_ingredients(&[
            template.clone().unwrap_or(Ingredient::Empty),
            base.clone(),
            addition.clone().unwrap_or(Ingredient::Empty),
        ]));
        SmithingRecipe::Transform(SmithingTransformRecipe {
            properties,
            template,
            base,
            addition,
            result: #result,
        })
    }}
}

fn generate_trim(value: &Value) -> TokenStream {
    let template = ingredient_tokens(field(value, "template"));
    let base = ingredient_tokens(field(value, "base"));
    let addition = ingredient_tokens(field(value, "addition"));
    let pattern = vanilla_ident(string_field(value, "pattern"));
    let properties = book_properties(quote! { RecipeBookCategoryKind::Smithing }, value);
    quote! {{
        let template = #template;
        let base = #base;
        let addition = #addition;
        let mut properties = #properties;
        properties.placement = Some(PlacementInfo::from_ingredients(&[
            template.clone(), base.clone(), addition.clone(),
        ]));
        SmithingRecipe::Trim(SmithingTrimRecipe {
            properties,
            template,
            base,
            addition,
            pattern: &vanilla_trim_patterns::#pattern,
        })
    }}
}
