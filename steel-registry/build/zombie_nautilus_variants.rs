use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
pub struct ZombieNautilusVariantJson {
    asset_id: Identifier,
    #[serde(default)]
    model: Option<String>,
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
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/zombie_nautilus_variant/"
    );

    let zombie_nautilus_variant_dir =
        "build_assets/builtin_datapacks/minecraft/data/minecraft/zombie_nautilus_variant";
    let mut zombie_nautilus_variants = Vec::new();

    // Read all zombie nautilus variant JSON files
    for entry in fs::read_dir(zombie_nautilus_variant_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let zombie_nautilus_variant_name =
                path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let zombie_nautilus_variant: ZombieNautilusVariantJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| {
                    panic!("Failed to parse {}: {}", zombie_nautilus_variant_name, e)
                });

            zombie_nautilus_variants.push((zombie_nautilus_variant_name, zombie_nautilus_variant));
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::zombie_nautilus_variant::{
            ZombieNautilusVariant, ZombieNautilusVariantRegistry, SpawnConditionEntry, BiomeCondition,
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;
    });

    // Generate static zombie nautilus variant definitions
    let mut register_stream = TokenStream::new();
    for (zombie_nautilus_variant_name, zombie_nautilus_variant) in &zombie_nautilus_variants {
        let zombie_nautilus_variant_ident = Ident::new(
            &zombie_nautilus_variant_name.to_shouty_snake_case(),
            Span::call_site(),
        );
        let zombie_nautilus_variant_name_str = zombie_nautilus_variant_name.clone();

        let key = quote! { Identifier::vanilla_static(#zombie_nautilus_variant_name_str) };
        let asset_id = generate_identifier(&zombie_nautilus_variant.asset_id);
        let model = generate_option(&zombie_nautilus_variant.model, |m| {
            let model_str = m.as_str();
            quote! { #model_str }
        });

        let spawn_conditions: Vec<_> = zombie_nautilus_variant
            .spawn_conditions
            .iter()
            .map(generate_spawn_condition_entry)
            .collect();

        stream.extend(quote! {
            pub static #zombie_nautilus_variant_ident: &ZombieNautilusVariant = &ZombieNautilusVariant {
                key: #key,
                asset_id: #asset_id,
                model: #model,
                spawn_conditions: &[#(#spawn_conditions),*],
            };
        });

        let zombie_nautilus_variant_ident = Ident::new(
            &zombie_nautilus_variant_name.to_shouty_snake_case(),
            Span::call_site(),
        );
        register_stream.extend(quote! {
            registry.register(#zombie_nautilus_variant_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_zombie_nautilus_variants(registry: &mut ZombieNautilusVariantRegistry) {
            #register_stream
        }
    });

    stream
}
