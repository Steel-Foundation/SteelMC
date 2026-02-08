use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
pub struct PigVariantJson {
    asset_id: Identifier,
    #[serde(default)]
    model: String,
    spawn_conditions: Vec<SpawnConditionEntry>,
}

#[derive(Deserialize, Debug)]
pub struct SpawnConditionEntry {
    priority: i32,
    #[serde(default)]
    condition: Option<BiomeCondition>,
}

#[derive(Deserialize, Debug)]
pub struct BiomeCondition {
    #[serde(rename = "type")]
    condition_type: String,
    biomes: String,
}

fn generate_identifier(resource: &Identifier) -> TokenStream {
    let namespace = resource.namespace.as_ref();
    let path = resource.path.as_ref();
    quote! { Identifier { namespace: Cow::Borrowed(#namespace), path: Cow::Borrowed(#path) } }
}

fn generate_option<T, F>(opt: &Option<T>, f: F) -> TokenStream
where
    F: FnOnce(&T) -> TokenStream,
{
    match opt {
        Some(val) => {
            let inner = f(val);
            quote! { Some(#inner) }
        }
        None => quote! { None },
    }
}

fn generate_pig_model_type(model: &str) -> TokenStream {
    match model {
        "cold" => quote! { PigModelType::Cold },
        _ => quote! { PigModelType::Normal },
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
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/pig_variant/"
    );

    let pig_variant_dir = "build_assets/builtin_datapacks/minecraft/data/minecraft/pig_variant";
    let mut pig_variants = Vec::new();

    // Read all pig variant JSON files
    for entry in fs::read_dir(pig_variant_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let pig_variant_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let pig_variant: PigVariantJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", pig_variant_name, e));

            pig_variants.push((pig_variant_name, pig_variant));
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::pig_variant::{
            PigVariant, PigVariantRegistry, PigModelType, SpawnConditionEntry, BiomeCondition,
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;
    });

    // Generate static pig variant definitions
    let mut register_stream = TokenStream::new();
    for (pig_variant_name, pig_variant) in &pig_variants {
        let pig_variant_ident =
            Ident::new(&pig_variant_name.to_shouty_snake_case(), Span::call_site());
        let pig_variant_name_str = pig_variant_name.clone();

        let key = quote! { Identifier::vanilla_static(#pig_variant_name_str) };
        let asset_id = generate_identifier(&pig_variant.asset_id);
        let model = generate_pig_model_type(&pig_variant.model);

        let spawn_conditions: Vec<_> = pig_variant
            .spawn_conditions
            .iter()
            .map(generate_spawn_condition_entry)
            .collect();

        stream.extend(quote! {
            pub static #pig_variant_ident: &PigVariant = &PigVariant {
                key: #key,
                asset_id: #asset_id,
                model: #model,
                spawn_conditions: &[#(#spawn_conditions),*],
            };
        });

        register_stream.extend(quote! {
            registry.register(#pig_variant_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_pig_variants(registry: &mut PigVariantRegistry) {
            #register_stream
        }
    });

    stream
}
