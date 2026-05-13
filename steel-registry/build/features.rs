//! Build-time codegen for configured and placed feature registries.

use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

#[allow(dead_code)]
#[path = "../src/feature/data.rs"]
mod feature_data;

use feature_data::{ConfiguredFeatureKind, PlacedFeatureData};

fn sorted_json_files(dir: &str) -> Vec<fs::DirEntry> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("{dir} missing: {err}"))
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    files.sort_by_key(|entry| entry.file_name());
    files
}

fn resource_name(entry: &fs::DirEntry) -> String {
    entry
        .path()
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_else(|| panic!("invalid feature file name: {:?}", entry.path()))
        .to_owned()
}

pub(crate) fn build_configured() -> TokenStream {
    let dir = "build_assets/builtin_datapacks/minecraft/worldgen/configured_feature";
    println!("cargo:rerun-if-changed={dir}");

    let mut entries = Vec::new();
    for entry in sorted_json_files(dir) {
        let name = resource_name(&entry);
        let path = entry.path();
        let content =
            fs::read_to_string(&path).unwrap_or_else(|err| panic!("failed to read {name}: {err}"));
        serde_json::from_str::<ConfiguredFeatureKind>(&content)
            .unwrap_or_else(|err| panic!("failed to parse configured feature {name}: {err}"));
        entries.push((name, path.to_string_lossy().replace('\\', "/")));
    }

    let mut stream = TokenStream::new();
    stream.extend(quote! {
        use crate::feature::{ConfiguredFeature, ConfiguredFeatureKind, ConfiguredFeatureRegistry};
        use steel_utils::Identifier;
        use std::sync::{LazyLock, OnceLock};
    });

    let mut register = TokenStream::new();
    for (name, path) in &entries {
        let ident = Ident::new(&name.to_shouty_snake_case(), Span::call_site());
        let include_path = format!("../../{path}");
        stream.extend(quote! {
            pub static #ident: LazyLock<ConfiguredFeature> = LazyLock::new(|| {
                let kind = match serde_json::from_str::<ConfiguredFeatureKind>(include_str!(#include_path)) {
                    Ok(kind) => kind,
                    Err(err) => panic!("failed to parse generated configured feature {}: {err}", #name),
                };
                ConfiguredFeature {
                    key: Identifier::vanilla_static(#name),
                    kind,
                    id: OnceLock::new(),
                }
            });
        });
        register.extend(quote! {
            registry.register(&#ident);
        });
    }

    stream.extend(quote! {
        pub fn register_configured_features(registry: &mut ConfiguredFeatureRegistry) {
            #register
        }
    });

    stream
}

pub(crate) fn build_placed() -> TokenStream {
    let dir = "build_assets/builtin_datapacks/minecraft/worldgen/placed_feature";
    println!("cargo:rerun-if-changed={dir}");

    let mut entries = Vec::new();
    for entry in sorted_json_files(dir) {
        let name = resource_name(&entry);
        let path = entry.path();
        let content =
            fs::read_to_string(&path).unwrap_or_else(|err| panic!("failed to read {name}: {err}"));
        serde_json::from_str::<PlacedFeatureData>(&content)
            .unwrap_or_else(|err| panic!("failed to parse placed feature {name}: {err}"));
        entries.push((name, path.to_string_lossy().replace('\\', "/")));
    }

    let mut stream = TokenStream::new();
    stream.extend(quote! {
        use crate::feature::{PlacedFeature, PlacedFeatureData, PlacedFeatureRegistry};
        use steel_utils::Identifier;
        use std::sync::{LazyLock, OnceLock};
    });

    let mut register = TokenStream::new();
    for (name, path) in &entries {
        let ident = Ident::new(&name.to_shouty_snake_case(), Span::call_site());
        let include_path = format!("../../{path}");
        stream.extend(quote! {
            pub static #ident: LazyLock<PlacedFeature> = LazyLock::new(|| {
                let data = match serde_json::from_str::<PlacedFeatureData>(include_str!(#include_path)) {
                    Ok(data) => data,
                    Err(err) => panic!("failed to parse generated placed feature {}: {err}", #name),
                };
                PlacedFeature {
                    key: Identifier::vanilla_static(#name),
                    data,
                    id: OnceLock::new(),
                }
            });
        });
        register.extend(quote! {
            registry.register(&#ident);
        });
    }

    stream.extend(quote! {
        pub fn register_placed_features(registry: &mut PlacedFeatureRegistry) {
            #register
        }
    });

    stream
}
