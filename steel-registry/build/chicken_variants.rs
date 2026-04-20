use crate::generator_functions::{generate_identifier, generate_option, read_variants_from_dir};
use crate::shared_structs::{BiomeCondition, SpawnConditionEntry};
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
pub struct ChickenVariantJson {
    asset_id: Identifier,
    baby_asset_id: Identifier,
    #[serde(default)]
    model: String,
    spawn_conditions: Vec<SpawnConditionEntry>,
}

fn generate_chicken_model_type(model: &str) -> TokenStream {
    match model {
        "cold" => quote! { ChickenModelType::Cold },
        _ => quote! { ChickenModelType::Normal },
    }
}

fn generate_biome_condition(condition: &BiomeCondition) -> TokenStream {
    let condition_type = condition.condition_type.as_str();
    let biomes = condition.biomes.as_str();

    quote! {
        BiomeCondition {
            condition_type: #condition_type,
            biomes: #biomes,
        }
    }
}

fn generate_spawn_condition_entry(entry: &SpawnConditionEntry) -> TokenStream {
    let priority = entry.priority;
    let condition = generate_option(&entry.condition, generate_biome_condition);

    quote! {
        SpawnConditionEntry {
            priority: #priority,
            condition: #condition,
        }
    }
}

pub(crate) fn build() -> TokenStream {
    let chicken_variants: Vec<(String, ChickenVariantJson)> =
        read_variants_from_dir("chicken_variant");

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::chicken_variant::{
            ChickenVariant, ChickenVariantRegistry, ChickenModelType, SpawnConditionEntry, BiomeCondition,
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;
    });

    // Generate static chicken variant definitions
    let mut register_stream = TokenStream::new();
    for (chicken_variant_name, chicken_variant) in &chicken_variants {
        let chicken_variant_ident = Ident::new(
            &chicken_variant_name.to_shouty_snake_case(),
            Span::call_site(),
        );
        let chicken_variant_name_str = chicken_variant_name.clone();

        let key = quote! { Identifier::vanilla_static(#chicken_variant_name_str) };
        let asset_id = generate_identifier(&chicken_variant.asset_id);
        let baby_asset_id = generate_identifier(&chicken_variant.baby_asset_id);
        let model = generate_chicken_model_type(&chicken_variant.model);

        let spawn_conditions: Vec<_> = chicken_variant
            .spawn_conditions
            .iter()
            .map(generate_spawn_condition_entry)
            .collect();

        stream.extend(quote! {
            pub static #chicken_variant_ident: ChickenVariant = ChickenVariant {
                key: #key,
                asset_id: #asset_id,
                baby_asset_id: #baby_asset_id,
                model: #model,
                spawn_conditions: &[#(#spawn_conditions),*],
            };
        });
        register_stream.extend(quote! {
            registry.register(&#chicken_variant_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_chicken_variants(registry: &mut ChickenVariantRegistry) {
            #register_stream
        }
    });

    stream
}
