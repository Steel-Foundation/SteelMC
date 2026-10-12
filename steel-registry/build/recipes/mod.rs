//! Build-time generation of typed vanilla recipe declarations.

use std::path::Path;

use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;

mod cooking;
mod crafting;
mod serializer;
mod shared;
mod smithing;
mod stonecutting;

use crate::generator_functions::{ResourceData, read_resources};
use serializer::RecipeSerializer;
use shared::{recipe_ident, string_field};

type RecipeGenerator = fn(&Value) -> TokenStream;

struct ParsedRecipe {
    name: String,
    value: Value,
}

impl ResourceData<Value> for ParsedRecipe {
    fn new_data(key: String, value: Value) -> Self {
        ParsedRecipe { name: key, value }
    }

    fn name() -> &'static str {
        "recipe"
    }
}

pub(crate) fn build() -> TokenStream {
    let recipe_dir = Path::new("../steel-utils/build_assets/builtin_datapacks/minecraft/recipe");
    println!("cargo:rerun-if-changed={}", recipe_dir.display());

    let mut recipes: Vec<ParsedRecipe> = read_resources(recipe_dir);
    // TODO: generate `minecraft:brewing` recipes once brewing stands are implemented.
    recipes.retain(|recipe| string_field(&recipe.value, "type") != "minecraft:brewing");
    recipes.sort_by(|left, right| left.name.cmp(&right.name));

    let declarations: Vec<_> = recipes.iter().map(generate_declaration).collect();
    let registrations: Vec<_> = recipes
        .iter()
        .map(|recipe| {
            let ident = recipe_ident(&recipe.name);
            quote! { registry.register(&#ident); }
        })
        .collect();

    quote! {
        use std::sync::LazyLock;

        use steel_utils::Identifier;

        use crate::{
            data_components::{DataComponentPatch, vanilla_components},
            item_stack_template::ItemStackTemplate,
            recipe::*,
            RegistryReference, vanilla_items, vanilla_mob_effects, vanilla_potions,
            vanilla_trim_patterns,
        };
        use crate::data_components::vanilla_components::{
            FireworkExplosionShape, PotionContents, SuspiciousStewEffect, SuspiciousStewEffects,
        };

        #(#declarations)*

        /// Registers every extracted vanilla recipe using its hardcoded static declaration.
        pub fn register_recipes(registry: &mut RecipeRegistry) {
            #(#registrations)*
        }
    }
}

fn generate_declaration(recipe: &ParsedRecipe) -> TokenStream {
    let serializer_identifier = string_field(&recipe.value, "type");
    let Some(serializer) = RecipeSerializer::from_identifier(serializer_identifier) else {
        panic!(
            "Unsupported extracted recipe type {serializer_identifier} for {}",
            recipe.name
        );
    };
    let ident = recipe_ident(&recipe.name);
    let name = &recipe.name;
    let data = serializer.generate_data(&recipe.value);
    let (rust_type, operational_type) = serializer.recipe_type_tokens();

    quote! {
        pub static #ident: LazyLock<#rust_type> = LazyLock::new(|| {
            Recipe::new(
                Identifier::vanilla_static(#name),
                &#operational_type,
                #data,
            )
        });
    }
}
