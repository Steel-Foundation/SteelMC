//! Build-time generation of typed vanilla recipe declarations.

use std::{fs, path::Path};

use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;

mod cooking;
mod crafting;
mod serializer;
mod shared;
mod smithing;
mod stonecutting;

use serializer::RecipeSerializer;
use shared::{recipe_ident, string_field};

type RecipeGenerator = fn(&Value) -> TokenStream;

struct ParsedRecipe {
    name: String,
    value: Value,
}

pub(crate) fn build() -> TokenStream {
    let recipe_dir = Path::new("../steel-utils/build_assets/builtin_datapacks/minecraft/recipe");
    println!("cargo:rerun-if-changed={}", recipe_dir.display());

    let mut recipes = read_recipes(recipe_dir);
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
            vanilla_items, vanilla_mob_effects, vanilla_trim_patterns,
        };
        use crate::data_components::vanilla_components::{
            FireworkExplosionShape, SuspiciousStewEffect, SuspiciousStewEffects,
        };

        #(#declarations)*

        /// Registers every extracted vanilla recipe using its hardcoded static declaration.
        pub fn register_recipes(registry: &mut RecipeRegistry) {
            #(#registrations)*
        }
    }
}

fn read_recipes(dir: &Path) -> Vec<ParsedRecipe> {
    let mut recipes = Vec::new();
    read_recipes_from(dir, dir, &mut recipes);
    recipes
}

fn read_recipes_from(root: &Path, dir: &Path, recipes: &mut Vec<ParsedRecipe>) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("Cannot read recipe directory {}: {error}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!(
                "Cannot read an entry in recipe directory {}: {error}",
                dir.display()
            )
        });
        let path = entry.path();
        if path.is_dir() {
            read_recipes_from(root, &path, recipes);
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let name = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .with_extension("")
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("Cannot read recipe {}: {error}", path.display()));
        let value = serde_json::from_str(&source)
            .unwrap_or_else(|error| panic!("Cannot parse recipe {}: {error}", path.display()));
        recipes.push(ParsedRecipe { name, value });
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
