use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
pub struct CowVariantJson {
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

fn generate_cow_model_type(model: &str) -> TokenStream {
    match model {
        "cold" => quote! { CowModelType::Cold },
        "warm" => quote! { CowModelType::Warm },
        _ => quote! { CowModelType::Normal },
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
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/cow_variant/"
    );

    let cow_variant_dir = "build_assets/builtin_datapacks/minecraft/data/minecraft/cow_variant";
    let mut cow_variants = Vec::new();

    // Read all cow variant JSON files
    for entry in fs::read_dir(cow_variant_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let cow_variant_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let cow_variant: CowVariantJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", cow_variant_name, e));

            cow_variants.push((cow_variant_name, cow_variant));
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::cow_variant::{
            CowVariant, CowVariantRegistry, CowModelType, SpawnConditionEntry, BiomeCondition,
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;
    });

    // Generate static cow variant definitions
    let mut register_stream = TokenStream::new();
    for (cow_variant_name, cow_variant) in &cow_variants {
        let cow_variant_ident =
            Ident::new(&cow_variant_name.to_shouty_snake_case(), Span::call_site());
        let cow_variant_name_str = cow_variant_name.clone();

        let key = quote! { Identifier::vanilla_static(#cow_variant_name_str) };
        let asset_id = generate_identifier(&cow_variant.asset_id);
        let model = generate_cow_model_type(&cow_variant.model);

        let spawn_conditions: Vec<_> = cow_variant
            .spawn_conditions
            .iter()
            .map(generate_spawn_condition_entry)
            .collect();

        stream.extend(quote! {
            pub static #cow_variant_ident: &CowVariant = &CowVariant {
                key: #key,
                asset_id: #asset_id,
                model: #model,
                spawn_conditions: &[#(#spawn_conditions),*],
            };
        });
        let cow_variant_ident =
            Ident::new(&cow_variant_name.to_shouty_snake_case(), Span::call_site());
        register_stream.extend(quote! {
            registry.register(#cow_variant_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_cow_variants(registry: &mut CowVariantRegistry) {
            #register_stream
        }
    });

    stream
}
