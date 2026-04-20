use crate::generator_functions::{generate_identifier, generate_option, read_variants_from_dir};
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
pub struct CatVariantJson {
    asset_id: Identifier,
    baby_asset_id: Identifier,
    spawn_conditions: Vec<SpawnConditionEntry>,
}

#[derive(Deserialize, Debug)]
pub struct SpawnConditionEntry {
    priority: i32,
    #[serde(default)]
    condition: Option<ConditionJson>,
}

#[derive(Deserialize, Debug)]
pub struct ConditionJson {
    #[serde(rename = "type")]
    condition_type: String,
    #[serde(default)]
    structures: Option<String>,
    #[serde(default)]
    biomes: Option<String>,
    #[serde(default)]
    range: Option<RangeJson>,
}

#[derive(Deserialize, Debug)]
pub struct RangeJson {
    #[serde(default)]
    min: Option<f32>,
    #[serde(default)]
    max: Option<f32>,
}

fn generate_spawn_condition(condition: &ConditionJson) -> TokenStream {
    match condition.condition_type.as_str() {
        "minecraft:structure" => {
            let structures = condition.structures.as_ref().unwrap().as_str();
            quote! {
                SpawnCondition::Structure {
                    structures: #structures,
                }
            }
        }
        "minecraft:moon_brightness" => {
            let range = condition.range.as_ref().unwrap();
            let min = generate_option(&range.min, |v| quote! { #v });
            let max = generate_option(&range.max, |v| quote! { #v });
            quote! {
                SpawnCondition::MoonBrightness {
                    min: #min,
                    max: #max,
                }
            }
        }
        "minecraft:biome" => {
            let biomes = condition.biomes.as_ref().unwrap().as_str();
            quote! {
                SpawnCondition::Biome {
                    biomes: #biomes,
                }
            }
        }
        _ => {
            panic!("Unknown condition type: {}", condition.condition_type);
        }
    }
}

fn generate_spawn_condition_entry(entry: &SpawnConditionEntry) -> TokenStream {
    let priority = entry.priority;
    let condition = generate_option(&entry.condition, generate_spawn_condition);

    quote! {
        SpawnConditionEntry {
            priority: #priority,
            condition: #condition,
        }
    }
}

pub(crate) fn build() -> TokenStream {
    let cat_variants: Vec<(String, CatVariantJson)> = read_variants_from_dir("cat_variant");

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::cat_variant::{
            CatVariant, CatVariantRegistry, SpawnConditionEntry, SpawnCondition,
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;
    });

    // Generate static cat variant definitions
    let mut register_stream = TokenStream::new();
    for (cat_variant_name, cat_variant) in &cat_variants {
        let cat_variant_ident =
            Ident::new(&cat_variant_name.to_shouty_snake_case(), Span::call_site());
        let cat_variant_name_str = cat_variant_name.clone();

        let key = quote! { Identifier::vanilla_static(#cat_variant_name_str) };
        let asset_id = generate_identifier(&cat_variant.asset_id);
        let baby_asset_id = generate_identifier(&cat_variant.baby_asset_id);

        let spawn_conditions: Vec<_> = cat_variant
            .spawn_conditions
            .iter()
            .map(generate_spawn_condition_entry)
            .collect();

        stream.extend(quote! {
            pub static #cat_variant_ident: CatVariant = CatVariant {
                key: #key,
                asset_id: #asset_id,
                baby_asset_id: #baby_asset_id,
                spawn_conditions: &[#(#spawn_conditions),*],
            };
        });
        register_stream.extend(quote! {
            registry.register(&#cat_variant_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_cat_variants(registry: &mut CatVariantRegistry) {
            #register_stream
        }
    });

    stream
}
