//! Generators for vanilla crafting recipe serializers.

use heck::ToUpperCamelCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde_json::Value;

use super::RecipeGenerator;
use super::shared::{
    array_field, book_properties, field, ingredient_tokens, integer_field, object_field,
    result_tokens,
};

pub(super) fn generator(path: &str) -> Option<RecipeGenerator> {
    Some(match path {
        "crafting_shaped" => generate_shaped,
        "crafting_shapeless" => generate_shapeless,
        "crafting_transmute" => generate_transmute,
        "crafting_dye" => generate_dye,
        "crafting_decorated_pot" => generate_decorated_pot,
        "crafting_imbue" => generate_imbue,
        "crafting_special_bannerduplicate" => generate_banner_duplicate,
        "crafting_special_bookcloning" => generate_book_cloning,
        "crafting_special_firework_rocket" => generate_firework_rocket,
        "crafting_special_firework_star" => generate_firework_star,
        "crafting_special_firework_star_fade" => generate_firework_star_fade,
        "crafting_special_mapextending" => generate_map_extending,
        "crafting_special_repairitem" => generate_repair,
        "crafting_special_shielddecoration" => generate_shield_decoration,
        _ => return None,
    })
}

fn generate_shaped(value: &Value) -> TokenStream {
    let pattern = array_field(value, "pattern");
    let key = object_field(value, "key");
    let height = pattern.len();
    let width = pattern
        .iter()
        .map(|row| {
            row.as_str()
                .unwrap_or_else(|| panic!("Shaped recipe pattern row is not a string: {row}"))
                .chars()
                .count()
        })
        .max()
        .unwrap_or(0);
    let mut ingredients = Vec::with_capacity(width * height);
    for row in pattern {
        let row = row
            .as_str()
            .unwrap_or_else(|| panic!("Shaped recipe pattern row is not a string: {row}"));
        for x in 0..width {
            let character = row.chars().nth(x).unwrap_or(' ');
            if character == ' ' {
                ingredients.push(quote! { Ingredient::Empty });
                continue;
            }
            let ingredient = key
                .get(&character.to_string())
                .unwrap_or_else(|| panic!("Shaped pattern references missing key {character}"));
            ingredients.push(ingredient_tokens(ingredient));
        }
    }
    let properties = crafting_properties(value);
    let result = result_tokens(field(value, "result"));
    quote! {
        CraftingRecipe::Shaped(ShapedRecipe::new(
            #properties,
            #width,
            #height,
            vec![#(#ingredients),*].into_boxed_slice(),
            #result,
        ))
    }
}

fn generate_shapeless(value: &Value) -> TokenStream {
    let ingredients: Vec<_> = array_field(value, "ingredients")
        .iter()
        .map(ingredient_tokens)
        .collect();
    let properties = crafting_properties(value);
    let result = result_tokens(field(value, "result"));
    quote! {
        CraftingRecipe::Shapeless(ShapelessRecipe::new(
            #properties,
            vec![#(#ingredients),*].into_boxed_slice(),
            #result,
        ))
    }
}

fn generate_transmute(value: &Value) -> TokenStream {
    let input = ingredient_tokens(field(value, "input"));
    let material = ingredient_tokens(field(value, "material"));
    let (minimum, maximum) = value.get("material_count").map_or((1, 1), |bounds| {
        (
            integer_field(bounds, "min") as usize,
            integer_field(bounds, "max") as usize,
        )
    });
    let add_count = value
        .get("add_material_count_to_result")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let properties = crafting_properties(value);
    let result = result_tokens(field(value, "result"));
    quote! {{
        let input = #input;
        let material = #material;
        let mut properties = #properties;
        let mut placement = Vec::with_capacity(1 + #maximum);
        placement.push(input.clone());
        placement.extend((0..#maximum).map(|_| material.clone()));
        properties.placement = Some(PlacementInfo::from_ingredients(&placement));
        CraftingRecipe::Transmute(TransmuteRecipe {
            properties,
            input,
            material,
            min_material_count: #minimum,
            max_material_count: #maximum,
            result: #result,
            add_material_count_to_result: #add_count,
        })
    }}
}

fn generate_dye(value: &Value) -> TokenStream {
    let target = ingredient_tokens(field(value, "target"));
    let dye = ingredient_tokens(field(value, "dye"));
    let properties = crafting_properties(value);
    let result = result_tokens(field(value, "result"));
    quote! {{
        let target = #target;
        let dye = #dye;
        let mut properties = #properties;
        properties.placement = Some(PlacementInfo::from_ingredients(&[target.clone(), dye.clone()]));
        CraftingRecipe::Dye(DyeRecipe { properties, target, dye, result: #result })
    }}
}

fn generate_decorated_pot(value: &Value) -> TokenStream {
    let back = ingredient_tokens(field(value, "back"));
    let left = ingredient_tokens(field(value, "left"));
    let right = ingredient_tokens(field(value, "right"));
    let front = ingredient_tokens(field(value, "front"));
    let result = result_tokens(field(value, "result"));
    quote! {
        CraftingRecipe::DecoratedPot(DecoratedPotRecipe {
            properties: RecipeProperties::special(),
            back: #back,
            left: #left,
            right: #right,
            front: #front,
            result: #result,
        })
    }
}

fn generate_imbue(value: &Value) -> TokenStream {
    let source = ingredient_tokens(field(value, "source"));
    let material = ingredient_tokens(field(value, "material"));
    let properties = crafting_properties(value);
    let result = result_tokens(field(value, "result"));
    quote! {{
        let source = #source;
        let material = #material;
        let mut properties = #properties;
        properties.placement = Some(PlacementInfo::from_ingredients(&[
            material.clone(), material.clone(), material.clone(), material.clone(),
            source.clone(), material.clone(), material.clone(), material.clone(), material.clone(),
        ]));
        CraftingRecipe::Imbue(ImbueRecipe { properties, source, material, result: #result })
    }}
}

fn generate_banner_duplicate(value: &Value) -> TokenStream {
    let banner = ingredient_tokens(field(value, "banner"));
    let result = result_tokens(field(value, "result"));
    quote! {
        CraftingRecipe::BannerDuplicate(BannerDuplicateRecipe {
            properties: RecipeProperties::special(),
            banner: #banner,
            result: #result,
        })
    }
}

fn generate_book_cloning(value: &Value) -> TokenStream {
    let source = ingredient_tokens(field(value, "source"));
    let material = ingredient_tokens(field(value, "material"));
    let (minimum, maximum) = value.get("allowed_generations").map_or((0, 1), |bounds| {
        (integer_field(bounds, "min"), integer_field(bounds, "max"))
    });
    let result = result_tokens(field(value, "result"));
    quote! {
        CraftingRecipe::BookCloning(BookCloningRecipe {
            properties: RecipeProperties::special(),
            source: #source,
            material: #material,
            min_generation: #minimum,
            max_generation: #maximum,
            result: #result,
        })
    }
}

fn generate_firework_rocket(value: &Value) -> TokenStream {
    let shell = ingredient_tokens(field(value, "shell"));
    let fuel = ingredient_tokens(field(value, "fuel"));
    let star = ingredient_tokens(field(value, "star"));
    let result = result_tokens(field(value, "result"));
    quote! {
        CraftingRecipe::FireworkRocket(FireworkRocketRecipe {
            properties: RecipeProperties::special(),
            shell: #shell,
            fuel: #fuel,
            star: #star,
            result: #result,
        })
    }
}

fn generate_firework_star(value: &Value) -> TokenStream {
    let shapes: Vec<_> = object_field(value, "shapes")
        .iter()
        .map(|(shape, ingredient)| {
            let variant = Ident::new(&shape.to_upper_camel_case(), Span::call_site());
            let ingredient = ingredient_tokens(ingredient);
            quote! { (FireworkExplosionShape::#variant, #ingredient) }
        })
        .collect();
    let trail = ingredient_tokens(field(value, "trail"));
    let twinkle = ingredient_tokens(field(value, "twinkle"));
    let fuel = ingredient_tokens(field(value, "fuel"));
    let dye = ingredient_tokens(field(value, "dye"));
    let result = result_tokens(field(value, "result"));
    quote! {
        CraftingRecipe::FireworkStar(FireworkStarRecipe {
            properties: RecipeProperties::special(),
            shapes: vec![#(#shapes),*].into_boxed_slice(),
            trail: #trail,
            twinkle: #twinkle,
            fuel: #fuel,
            dye: #dye,
            result: #result,
        })
    }
}

fn generate_firework_star_fade(value: &Value) -> TokenStream {
    let target = ingredient_tokens(field(value, "target"));
    let dye = ingredient_tokens(field(value, "dye"));
    let result = result_tokens(field(value, "result"));
    quote! {
        CraftingRecipe::FireworkStarFade(FireworkStarFadeRecipe {
            properties: RecipeProperties::special(),
            target: #target,
            dye: #dye,
            result: #result,
        })
    }
}

fn generate_map_extending(value: &Value) -> TokenStream {
    let map = ingredient_tokens(field(value, "map"));
    let material = ingredient_tokens(field(value, "material"));
    let result = result_tokens(field(value, "result"));
    quote! {
        CraftingRecipe::MapExtending(MapExtendingRecipe {
            properties: RecipeProperties::special(),
            map: #map,
            material: #material,
            result: #result,
        })
    }
}

fn generate_repair(_value: &Value) -> TokenStream {
    quote! {
        CraftingRecipe::RepairItem(RepairItemRecipe {
            properties: RecipeProperties::special(),
        })
    }
}

fn generate_shield_decoration(value: &Value) -> TokenStream {
    let banner = ingredient_tokens(field(value, "banner"));
    let target = ingredient_tokens(field(value, "target"));
    let result = result_tokens(field(value, "result"));
    quote! {
        CraftingRecipe::ShieldDecoration(ShieldDecorationRecipe {
            properties: RecipeProperties::special(),
            banner: #banner,
            target: #target,
            result: #result,
        })
    }
}

fn crafting_properties(value: &Value) -> TokenStream {
    let category = match value
        .get("category")
        .and_then(Value::as_str)
        .unwrap_or("misc")
    {
        "building" => quote! { CraftingBookCategory::Building },
        "redstone" => quote! { CraftingBookCategory::Redstone },
        "equipment" => quote! { CraftingBookCategory::Equipment },
        "misc" => quote! { CraftingBookCategory::Misc },
        category => panic!("Unknown crafting book category {category}"),
    };
    book_properties(
        quote! { RecipeBookCategoryKind::Crafting(#category) },
        value,
    )
}
