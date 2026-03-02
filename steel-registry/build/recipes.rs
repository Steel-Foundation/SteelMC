//! Build script for generating vanilla recipe definitions.
//!
//! This module generates recipe definitions using a hybrid approach:
//! - `LazyLock` for the RECIPES struct (required because ITEMS uses LazyLock)
//! - `Box::leak` to create `&'static [Ingredient]` slices at runtime
//! - `#[inline(never)]` creator functions to prevent stack overflow
//!
//! The `Box::leak` pattern is intentional: vanilla recipes live for the entire
//! program lifetime, so leaking the memory is correct. This gives us zero-cost
//! access to recipe data after initialization.

use std::{fs, path::Path};

use heck::ToSnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use rustc_hash::FxHashMap;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Debug)]
struct RecipeJson {
    #[serde(rename = "type")]
    recipe_type: String,
    #[serde(default)]
    category: Option<String>,
    // Shaped recipe fields
    #[serde(default)]
    key: Option<serde_json::Map<String, Value>>,
    #[serde(default)]
    pattern: Option<Vec<String>>,
    // Shapeless recipe fields
    #[serde(default)]
    ingredients: Option<Vec<Value>>,
    // Common fields
    #[serde(default)]
    result: Option<RecipeResult>,
    #[serde(default)]
    show_notification: Option<bool>,
}

#[derive(Deserialize, Debug)]
struct RecipeResult {
    id: String,
    #[serde(default = "default_count")]
    count: i32,
}

fn default_count() -> i32 {
    1
}

/// Represents a parsed ingredient from JSON.
#[derive(Clone)]
enum ParsedIngredient {
    Empty,
    Item(String),        // item identifier
    Tag(String),         // tag identifier
    Choice(Vec<String>), // list of item identifiers
}

/// Parses an ingredient from a JSON value.
fn parse_ingredient(value: &Value) -> ParsedIngredient {
    match value {
        Value::String(s) => {
            if let Some(tag) = s.strip_prefix('#') {
                let tag_id = tag.strip_prefix("minecraft:").unwrap_or(tag);
                ParsedIngredient::Tag(tag_id.to_string())
            } else {
                let item_id = s.strip_prefix("minecraft:").unwrap_or(s);
                ParsedIngredient::Item(item_id.to_string())
            }
        }
        Value::Array(arr) => {
            let items: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| {
                    let item_id = s.strip_prefix("minecraft:").unwrap_or(s);
                    item_id.to_string()
                })
                .collect();
            ParsedIngredient::Choice(items)
        }
        Value::Object(obj) => {
            if let Some(item) = obj.get("item").and_then(|v| v.as_str()) {
                let item_id = item.strip_prefix("minecraft:").unwrap_or(item);
                ParsedIngredient::Item(item_id.to_string())
            } else if let Some(tag) = obj.get("tag").and_then(|v| v.as_str()) {
                let tag_id = tag.strip_prefix("minecraft:").unwrap_or(tag);
                ParsedIngredient::Tag(tag_id.to_string())
            } else {
                ParsedIngredient::Empty
            }
        }
        _ => ParsedIngredient::Empty,
    }
}

struct ShapedRecipeData {
    name: String,
    ident: Ident,
    category: TokenStream,
    width: usize,
    height: usize,
    pattern_data: Vec<ParsedIngredient>,
    result_item_ident: Ident,
    result_count: i32,
    show_notification: bool,
    symmetrical: bool,
}

struct ShapelessRecipeData {
    name: String,
    ident: Ident,
    category: TokenStream,
    ingredient_data: Vec<ParsedIngredient>,
    result_item_ident: Ident,
    result_count: i32,
}

/// Parses a shaped recipe from JSON.
fn parse_shaped_recipe(recipe_name: &str, recipe: &RecipeJson) -> Option<ShapedRecipeData> {
    let pattern = recipe.pattern.as_ref()?;
    let key = recipe.key.as_ref()?;
    let result = recipe.result.as_ref()?;

    // Calculate width and height from pattern
    let height = pattern.len();
    let width = pattern
        .iter()
        .map(|row| row.chars().count())
        .max()
        .unwrap_or(0);

    // Build ingredient map from key
    let mut ingredient_map: FxHashMap<char, ParsedIngredient> = FxHashMap::default();
    ingredient_map.insert(' ', ParsedIngredient::Empty);

    for (key_char, value) in key {
        if let Some(c) = key_char.chars().next() {
            ingredient_map.insert(c, parse_ingredient(value));
        }
    }

    // Build pattern vector and character grid for symmetry check
    let mut pattern_data = Vec::new();
    let mut char_grid: Vec<char> = Vec::new();
    for row in pattern {
        // Pad row to width
        let padded: String = format!("{:width$}", row, width = width);
        for c in padded.chars() {
            char_grid.push(c);
            let ingredient = ingredient_map
                .get(&c)
                .cloned()
                .unwrap_or(ParsedIngredient::Empty);
            pattern_data.push(ingredient);
        }
    }

    // Check horizontal symmetry using the character grid
    let symmetrical = is_pattern_symmetrical(width, height, &char_grid);

    // Result item
    let result_item_id = result.id.strip_prefix("minecraft:").unwrap_or(&result.id);
    let result_item_ident = Ident::new(result_item_id, Span::call_site());

    // Category
    let category_str = recipe.category.as_deref().unwrap_or("misc");
    let category = match category_str {
        "building" => quote! { CraftingCategory::Building },
        "redstone" => quote! { CraftingCategory::Redstone },
        "equipment" => quote! { CraftingCategory::Equipment },
        _ => quote! { CraftingCategory::Misc },
    };

    let snake_name = recipe_name.to_snake_case();

    Some(ShapedRecipeData {
        name: recipe_name.to_string(),
        ident: Ident::new(&snake_name, Span::call_site()),
        category,
        width,
        height,
        pattern_data,
        result_item_ident,
        result_count: result.count,
        show_notification: recipe.show_notification.unwrap_or(true),
        symmetrical,
    })
}

/// Checks if a pattern is horizontally symmetric.
fn is_pattern_symmetrical(width: usize, height: usize, chars: &[char]) -> bool {
    if width == 0 {
        return true;
    }
    for y in 0..height {
        for x in 0..width / 2 {
            let left = chars[y * width + x];
            let right = chars[y * width + (width - 1 - x)];
            if left != right {
                return false;
            }
        }
    }
    true
}

/// Parses a shapeless recipe from JSON.
fn parse_shapeless_recipe(recipe_name: &str, recipe: &RecipeJson) -> Option<ShapelessRecipeData> {
    let ingredients = recipe.ingredients.as_ref()?;
    let result = recipe.result.as_ref()?;

    // Build ingredients vector
    let ingredient_data: Vec<ParsedIngredient> = ingredients.iter().map(parse_ingredient).collect();

    // Result item
    let result_item_id = result.id.strip_prefix("minecraft:").unwrap_or(&result.id);
    let result_item_ident = Ident::new(result_item_id, Span::call_site());

    // Category
    let category_str = recipe.category.as_deref().unwrap_or("misc");
    let category = match category_str {
        "building" => quote! { CraftingCategory::Building },
        "redstone" => quote! { CraftingCategory::Redstone },
        "equipment" => quote! { CraftingCategory::Equipment },
        _ => quote! { CraftingCategory::Misc },
    };

    let snake_name = recipe_name.to_snake_case();

    Some(ShapelessRecipeData {
        name: recipe_name.to_string(),
        ident: Ident::new(&snake_name, Span::call_site()),
        category,
        ingredient_data,
        result_item_ident,
        result_count: result.count,
    })
}

/// Generates a TokenStream for an ingredient.
/// For Choice ingredients, uses Box::leak to create a static slice.
fn generate_ingredient_tokens(ingredient: &ParsedIngredient) -> TokenStream {
    match ingredient {
        ParsedIngredient::Empty => quote! { Ingredient::Empty },
        ParsedIngredient::Item(item_id) => {
            let item_ident = Ident::new(item_id, Span::call_site());
            quote! { Ingredient::Item(&ITEMS.#item_ident) }
        }
        ParsedIngredient::Tag(tag_id) => {
            quote! { Ingredient::Tag(Identifier::vanilla_static(#tag_id)) }
        }
        ParsedIngredient::Choice(items) => {
            let item_refs: Vec<TokenStream> = items
                .iter()
                .map(|item_id| {
                    let item_ident = Ident::new(item_id, Span::call_site());
                    quote! { &ITEMS.#item_ident }
                })
                .collect();
            // Use Box::leak to create a static slice for Choice items
            quote! {
                Ingredient::Choice(Box::leak(Box::new([#(#item_refs),*])))
            }
        }
    }
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/recipe/"
    );

    let recipe_dir = "build_assets/builtin_datapacks/minecraft/data/minecraft/recipe";

    let mut shaped_recipes: Vec<ShapedRecipeData> = Vec::new();
    let mut shapeless_recipes: Vec<ShapelessRecipeData> = Vec::new();

    // Read all recipe files
    fn read_recipes(
        dir: &Path,
        shaped: &mut Vec<ShapedRecipeData>,
        shapeless: &mut Vec<ShapelessRecipeData>,
    ) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();

            if path.is_dir() {
                read_recipes(&path, shaped, shapeless);
            } else if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let recipe_name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");

                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                let recipe: RecipeJson = match serde_json::from_str(&content) {
                    Ok(r) => r,
                    Err(_) => continue,
                };

                match recipe.recipe_type.as_str() {
                    "minecraft:crafting_shaped" => {
                        if let Some(r) = parse_shaped_recipe(recipe_name, &recipe) {
                            shaped.push(r);
                        }
                    }
                    "minecraft:crafting_shapeless" => {
                        if let Some(r) = parse_shapeless_recipe(recipe_name, &recipe) {
                            shapeless.push(r);
                        }
                    }
                    // Skip other recipe types for now (smelting, stonecutting, smithing, etc.)
                    _ => {}
                }
            }
        }
    }

    read_recipes(
        Path::new(recipe_dir),
        &mut shaped_recipes,
        &mut shapeless_recipes,
    );

    // Generate individual creator functions for each shaped recipe.
    // Each function creates just one recipe in its own stack frame,
    // preventing stack overflow from large struct literals.
    // Uses Box::leak to create &'static [Ingredient] slices.
    let shaped_creator_fns: Vec<TokenStream> = shaped_recipes
        .iter()
        .map(|r| {
            let fn_ident = Ident::new(&format!("create_shaped_{}", r.ident), Span::call_site());
            let name = &r.name;
            let category = &r.category;
            let width = r.width;
            let height = r.height;
            let result_item_ident = &r.result_item_ident;
            let result_count = r.result_count;
            let show_notification = r.show_notification;
            let symmetrical = r.symmetrical;

            let pattern_tokens: Vec<TokenStream> = r
                .pattern_data
                .iter()
                .map(generate_ingredient_tokens)
                .collect();

            quote! {
                #[inline(never)]
                fn #fn_ident() -> ShapedRecipe {
                    // Box::leak creates a &'static [Ingredient] from the Vec.
                    // This is intentional: vanilla recipes live forever.
                    let pattern: &'static [Ingredient] = Box::leak(
                        vec![#(#pattern_tokens),*].into_boxed_slice()
                    );
                    ShapedRecipe {
                        id: Identifier::vanilla_static(#name),
                        category: #category,
                        width: #width,
                        height: #height,
                        pattern,
                        result: RecipeResult {
                            item: &ITEMS.#result_item_ident,
                            count: #result_count,
                        },
                        show_notification: #show_notification,
                        symmetrical: #symmetrical,
                    }
                }
            }
        })
        .collect();

    // Generate individual creator functions for each shapeless recipe.
    let shapeless_creator_fns: Vec<TokenStream> = shapeless_recipes
        .iter()
        .map(|r| {
            let fn_ident = Ident::new(&format!("create_shapeless_{}", r.ident), Span::call_site());
            let name = &r.name;
            let category = &r.category;
            let result_item_ident = &r.result_item_ident;
            let result_count = r.result_count;

            let ingredient_tokens: Vec<TokenStream> = r
                .ingredient_data
                .iter()
                .map(generate_ingredient_tokens)
                .collect();

            quote! {
                #[inline(never)]
                fn #fn_ident() -> ShapelessRecipe {
                    // Box::leak creates a &'static [Ingredient] from the Vec.
                    // This is intentional: vanilla recipes live forever.
                    let ingredients: &'static [Ingredient] = Box::leak(
                        vec![#(#ingredient_tokens),*].into_boxed_slice()
                    );
                    ShapelessRecipe {
                        id: Identifier::vanilla_static(#name),
                        category: #category,
                        ingredients,
                        result: RecipeResult {
                            item: &ITEMS.#result_item_ident,
                            count: #result_count,
                        },
                    }
                }
            }
        })
        .collect();

    // Generate struct fields
    let shaped_fields: Vec<TokenStream> = shaped_recipes
        .iter()
        .map(|r| {
            let ident = &r.ident;
            quote! { pub #ident: ShapedRecipe, }
        })
        .collect();

    let shapeless_fields: Vec<TokenStream> = shapeless_recipes
        .iter()
        .map(|r| {
            let ident = &r.ident;
            quote! { pub #ident: ShapelessRecipe, }
        })
        .collect();

    // Generate field initializers that call the creator functions
    let shaped_field_inits: Vec<TokenStream> = shaped_recipes
        .iter()
        .map(|r| {
            let ident = &r.ident;
            let fn_ident = Ident::new(&format!("create_shaped_{}", r.ident), Span::call_site());
            quote! { #ident: #fn_ident(), }
        })
        .collect();

    let shapeless_field_inits: Vec<TokenStream> = shapeless_recipes
        .iter()
        .map(|r| {
            let ident = &r.ident;
            let fn_ident = Ident::new(&format!("create_shapeless_{}", r.ident), Span::call_site());
            quote! { #ident: #fn_ident(), }
        })
        .collect();

    // Generate registration calls
    let shaped_registers: Vec<TokenStream> = shaped_recipes
        .iter()
        .map(|r| {
            let ident = &r.ident;
            quote! { registry.register_shaped(&RECIPES.shaped.#ident); }
        })
        .collect();

    let shapeless_registers: Vec<TokenStream> = shapeless_recipes
        .iter()
        .map(|r| {
            let ident = &r.ident;
            quote! { registry.register_shapeless(&RECIPES.shapeless.#ident); }
        })
        .collect();

    quote! {
        use crate::{
            recipe::{
                CraftingCategory, Ingredient, RecipeRegistry, RecipeResult,
                ShapedRecipe, ShapelessRecipe,
            },
            vanilla_items::ITEMS,
        };
        use steel_utils::Identifier;
        use std::sync::LazyLock;

        /// Global vanilla recipes instance.
        ///
        /// Uses `LazyLock` for thread-safe lazy initialization.
        /// Recipe data (patterns/ingredients) uses `Box::leak` to create
        /// `&'static` slices, providing zero-cost access after initialization.
        pub static RECIPES: LazyLock<Recipes> = LazyLock::new(Recipes::init);

        pub struct ShapedRecipes {
            #(#shaped_fields)*
        }

        pub struct ShapelessRecipes {
            #(#shapeless_fields)*
        }

        pub struct Recipes {
            pub shaped: ShapedRecipes,
            pub shapeless: ShapelessRecipes,
        }

        // Individual recipe creator functions.
        //
        // Each function is marked `#[inline(never)]` to ensure it gets its own
        // stack frame. This prevents stack overflow that would occur if all
        // recipes were initialized in a single large struct literal.
        //
        // Each function uses `Box::leak` to convert the ingredient Vec into
        // a `&'static [Ingredient]`. This is intentional and correct:
        // - Vanilla recipes live for the entire program lifetime
        // - The leaked memory is a one-time cost at startup
        // - Access to recipe data after init is zero-cost (just pointer + length)
        #(#shaped_creator_fns)*
        #(#shapeless_creator_fns)*

        impl Recipes {
            fn init() -> Self {
                Self {
                    shaped: ShapedRecipes {
                        #(#shaped_field_inits)*
                    },
                    shapeless: ShapelessRecipes {
                        #(#shapeless_field_inits)*
                    },
                }
            }
        }

        /// Registers all vanilla recipes with the recipe registry.
        pub fn register_recipes(registry: &mut RecipeRegistry) {
            // Force initialization of RECIPES
            let _ = &*RECIPES;
            #(#shaped_registers)*
            #(#shapeless_registers)*
        }
    }
}
