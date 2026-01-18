//! Build script for generating vanilla recipe definitions.

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

/// Generates the ingredient token stream from a JSON value.
fn generate_ingredient(value: &Value) -> TokenStream {
    match value {
        Value::String(s) => {
            if let Some(tag) = s.strip_prefix('#') {
                // Tag reference
                let tag_id = tag.strip_prefix("minecraft:").unwrap_or(tag);
                quote! { Ingredient::Tag(Identifier::vanilla_static(#tag_id)) }
            } else {
                // Single item
                let item_id = s.strip_prefix("minecraft:").unwrap_or(s);
                let item_ident = Ident::new(item_id, Span::call_site());
                quote! { Ingredient::Item(&ITEMS.#item_ident) }
            }
        }
        Value::Array(arr) => {
            // Choice of items
            let items: Vec<TokenStream> = arr
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| {
                    let item_id = s.strip_prefix("minecraft:").unwrap_or(s);
                    let item_ident = Ident::new(item_id, Span::call_site());
                    quote! { &ITEMS.#item_ident }
                })
                .collect();
            quote! { Ingredient::Choice(vec![#(#items),*]) }
        }
        Value::Object(obj) => {
            // Object with "item" or "tag" key (older format)
            if let Some(item) = obj.get("item").and_then(|v| v.as_str()) {
                let item_id = item.strip_prefix("minecraft:").unwrap_or(item);
                let item_ident = Ident::new(item_id, Span::call_site());
                quote! { Ingredient::Item(&ITEMS.#item_ident) }
            } else if let Some(tag) = obj.get("tag").and_then(|v| v.as_str()) {
                let tag_id = tag.strip_prefix("minecraft:").unwrap_or(tag);
                quote! { Ingredient::Tag(Identifier::vanilla_static(#tag_id)) }
            } else {
                quote! { Ingredient::Empty }
            }
        }
        _ => quote! { Ingredient::Empty },
    }
}

struct ShapedRecipeData {
    name: String,
    ident: Ident,
    category: TokenStream,
    width: usize,
    height: usize,
    pattern_tokens: Vec<TokenStream>,
    result_item_ident: Ident,
    result_count: i32,
    show_notification: bool,
    symmetrical: bool,
}

struct ShapelessRecipeData {
    name: String,
    ident: Ident,
    category: TokenStream,
    ingredient_tokens: Vec<TokenStream>,
    result_item_ident: Ident,
    result_count: i32,
}

/// Generates a shaped recipe.
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
    let mut ingredient_map: FxHashMap<char, TokenStream> = FxHashMap::default();
    ingredient_map.insert(' ', quote! { Ingredient::Empty });

    for (key_char, value) in key {
        if let Some(c) = key_char.chars().next() {
            ingredient_map.insert(c, generate_ingredient(value));
        }
    }

    // Build pattern vector and character grid for symmetry check
    let mut pattern_tokens = Vec::new();
    let mut char_grid: Vec<char> = Vec::new();
    for row in pattern {
        // Pad row to width
        let padded: String = format!("{:width$}", row, width = width);
        for c in padded.chars() {
            char_grid.push(c);
            let ingredient = ingredient_map
                .get(&c)
                .cloned()
                .unwrap_or_else(|| quote! { Ingredient::Empty });
            pattern_tokens.push(ingredient);
        }
    }

    // Check horizontal symmetry using the character grid
    // A pattern is symmetric if for each row, the left half mirrors the right half
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

    Some(ShapedRecipeData {
        name: recipe_name.to_string(),
        ident: Ident::new(&recipe_name.to_snake_case(), Span::call_site()),
        category,
        width,
        height,
        pattern_tokens,
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

/// Generates a shapeless recipe.
fn parse_shapeless_recipe(recipe_name: &str, recipe: &RecipeJson) -> Option<ShapelessRecipeData> {
    let ingredients = recipe.ingredients.as_ref()?;
    let result = recipe.result.as_ref()?;

    // Build ingredients vector
    let ingredient_tokens: Vec<TokenStream> = ingredients.iter().map(generate_ingredient).collect();

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

    Some(ShapelessRecipeData {
        name: recipe_name.to_string(),
        ident: Ident::new(&recipe_name.to_snake_case(), Span::call_site()),
        category,
        ingredient_tokens,
        result_item_ident,
        result_count: result.count,
    })
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

    // Generate recipe initializers
    let shaped_inits: Vec<TokenStream> = shaped_recipes
        .iter()
        .map(|r| {
            let ident = &r.ident;
            let name = &r.name;
            let category = &r.category;
            let width = r.width;
            let height = r.height;
            let pattern_tokens = &r.pattern_tokens;
            let result_item_ident = &r.result_item_ident;
            let result_count = r.result_count;
            let show_notification = r.show_notification;
            let symmetrical = r.symmetrical;

            quote! {
                #ident: ShapedRecipe {
                    id: Identifier::vanilla_static(#name),
                    category: #category,
                    width: #width,
                    height: #height,
                    pattern: vec![#(#pattern_tokens),*],
                    result: RecipeResult {
                        item: &ITEMS.#result_item_ident,
                        count: #result_count,
                    },
                    show_notification: #show_notification,
                    symmetrical: #symmetrical,
                },
            }
        })
        .collect();

    let shapeless_inits: Vec<TokenStream> = shapeless_recipes
        .iter()
        .map(|r| {
            let ident = &r.ident;
            let name = &r.name;
            let category = &r.category;
            let ingredient_tokens = &r.ingredient_tokens;
            let result_item_ident = &r.result_item_ident;
            let result_count = r.result_count;

            quote! {
                #ident: ShapelessRecipe {
                    id: Identifier::vanilla_static(#name),
                    category: #category,
                    ingredients: vec![#(#ingredient_tokens),*],
                    result: RecipeResult {
                        item: &ITEMS.#result_item_ident,
                        count: #result_count,
                    },
                },
            }
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

        impl Recipes {
            fn init() -> Self {
                Self {
                    shaped: ShapedRecipes {
                        #(#shaped_inits)*
                    },
                    shapeless: ShapelessRecipes {
                        #(#shapeless_inits)*
                    },
                }
            }
        }

        pub fn register_recipes(registry: &mut RecipeRegistry) {
            // Force initialization of RECIPES
            let _ = &*RECIPES;
            #(#shaped_registers)*
            #(#shapeless_registers)*
        }
    }
}
