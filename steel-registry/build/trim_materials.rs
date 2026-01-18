use rustc_hash::FxHashMap;
use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
pub struct TrimMaterialJson {
    asset_name: String,
    description: StyledTextComponent,
    #[serde(default)]
    override_armor_assets: FxHashMap<Identifier, String>,
}

#[derive(Deserialize, Debug)]
pub struct StyledTextComponent {
    translate: String,
    #[serde(default)]
    color: Option<String>,
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

fn generate_hashmap_resource_string(map: &FxHashMap<Identifier, String>) -> TokenStream {
    if map.is_empty() {
        return quote! { rustc_hash::FxHashMap::default() };
    }
    let entries: Vec<_> = map
        .iter()
        .map(|(k, v)| {
            let key = generate_identifier(k);
            quote! { (#key, #v.to_string()) }
        })
        .collect();
    quote! { rustc_hash::FxHashMap::from_iter([#(#entries),*]) }
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/trim_material/"
    );

    let trim_material_dir = "build_assets/builtin_datapacks/minecraft/data/minecraft/trim_material";
    let mut trim_materials = Vec::new();

    // Read all trim material JSON files
    for entry in fs::read_dir(trim_material_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let trim_material_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let trim_material: TrimMaterialJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", trim_material_name, e));

            trim_materials.push((trim_material_name, trim_material));
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::trim_material::{
            TrimMaterial, TrimMaterialRegistry, StyledTextComponent,
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;
        use std::sync::LazyLock;
        use rustc_hash::FxHashMap;
    });

    // Generate static trim material definitions
    for (trim_material_name, trim_material) in &trim_materials {
        let trim_material_ident = Ident::new(
            &trim_material_name.to_shouty_snake_case(),
            Span::call_site(),
        );
        let trim_material_name_str = trim_material_name.clone();

        let key = quote! { Identifier::vanilla_static(#trim_material_name_str) };
        let asset_name = &trim_material.asset_name;
        let translate = &trim_material.description.translate;
        let color = generate_option(&trim_material.description.color, |s| {
            let val = s.as_str();
            quote! { #val.to_string() }
        });
        let override_armor_assets =
            generate_hashmap_resource_string(&trim_material.override_armor_assets);

        stream.extend(quote! {
            pub static #trim_material_ident: LazyLock<TrimMaterial> = LazyLock::new(|| TrimMaterial {
                key: #key,
                asset_name: #asset_name.to_string(),
                description: StyledTextComponent {
                    translate: #translate.to_string(),
                    color: #color,
                },
                override_armor_assets: #override_armor_assets,
            });
        });
    }

    // Generate registration function
    let mut register_stream = TokenStream::new();
    for (trim_material_name, _) in &trim_materials {
        let trim_material_ident = Ident::new(
            &trim_material_name.to_shouty_snake_case(),
            Span::call_site(),
        );
        register_stream.extend(quote! {
            registry.register(&#trim_material_ident, #trim_material_ident.key.clone());
        });
    }

    stream.extend(quote! {
        pub fn register_trim_materials(registry: &mut TrimMaterialRegistry) {
            #register_stream
        }
    });

    stream
}
