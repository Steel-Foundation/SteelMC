//! Generator metadata and data tokens for vanilla cooking serializers.

use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;

use super::shared::{book_properties, field, ingredient_tokens, result_tokens};

#[derive(Clone, Copy)]
pub(super) enum CookingSerializer {
    Smelting,
    Blasting,
    Smoking,
    CampfireCooking,
}

impl CookingSerializer {
    pub(super) fn from_path(path: &str) -> Option<Self> {
        Some(match path {
            "smelting" => Self::Smelting,
            "blasting" => Self::Blasting,
            "smoking" => Self::Smoking,
            "campfire_cooking" => Self::CampfireCooking,
            _ => return None,
        })
    }

    pub(super) fn generate(self, value: &Value) -> TokenStream {
        let ingredient = ingredient_tokens(field(value, "ingredient"));
        let result = result_tokens(field(value, "result"));
        let properties = cooking_properties(value);
        let experience = value
            .get("experience")
            .and_then(Value::as_f64)
            .unwrap_or(0.0) as f32;
        let cooking_time = value
            .get("cookingtime")
            .and_then(Value::as_i64)
            .map_or(self.default_cooking_time(), |time| time as i32);
        quote! {{
            let ingredient = #ingredient;
            let mut properties = #properties;
            properties.placement = Some(PlacementInfo::from_ingredient(ingredient.clone()));
            CookingRecipe {
                properties,
                ingredient,
                result: #result,
                experience: #experience,
                cooking_time: #cooking_time,
            }
        }}
    }

    pub(super) fn recipe_type_tokens(self) -> TokenStream {
        match self {
            Self::Smelting => quote! { SMELTING },
            Self::Blasting => quote! { BLASTING },
            Self::Smoking => quote! { SMOKING },
            Self::CampfireCooking => quote! { CAMPFIRE_COOKING },
        }
    }

    const fn default_cooking_time(self) -> i32 {
        match self {
            Self::Smelting => 200,
            Self::Blasting | Self::Smoking | Self::CampfireCooking => 100,
        }
    }
}

fn cooking_properties(value: &Value) -> TokenStream {
    let category = match value
        .get("category")
        .and_then(Value::as_str)
        .unwrap_or("misc")
    {
        "food" => quote! { CookingBookCategory::Food },
        "blocks" => quote! { CookingBookCategory::Blocks },
        "misc" => quote! { CookingBookCategory::Misc },
        category => panic!("Unknown cooking book category {category}"),
    };
    book_properties(quote! { RecipeBookCategoryKind::Cooking(#category) }, value)
}
