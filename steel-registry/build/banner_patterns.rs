use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
pub struct BannerPatternJson {
    asset_id: Identifier,
    translation_key: String,
}

fn generate_identifier(resource: &Identifier) -> TokenStream {
    let namespace = resource.namespace.as_ref();
    let path = resource.path.as_ref();
    quote! { Identifier { namespace: Cow::Borrowed(#namespace), path: Cow::Borrowed(#path) } }
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/banner_pattern/"
    );

    let banner_pattern_dir =
        "build_assets/builtin_datapacks/minecraft/data/minecraft/banner_pattern";
    let mut banner_patterns = Vec::new();

    // Read all banner pattern JSON files
    for entry in fs::read_dir(banner_pattern_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let banner_pattern_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let banner_pattern: BannerPatternJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", banner_pattern_name, e));

            banner_patterns.push((banner_pattern_name, banner_pattern));
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::banner_pattern::{BannerPattern, BannerPatternRegistry};
        use steel_utils::Identifier;
        use std::borrow::Cow;
    });

    // Generate static banner pattern definitions
    for (banner_pattern_name, banner_pattern) in &banner_patterns {
        let banner_pattern_ident = Ident::new(
            &banner_pattern_name.to_shouty_snake_case(),
            Span::call_site(),
        );
        let banner_pattern_name_str = banner_pattern_name.clone();

        let key = quote! { Identifier::vanilla_static(#banner_pattern_name_str) };
        let asset_id = generate_identifier(&banner_pattern.asset_id);
        let translation_key = banner_pattern.translation_key.as_str();

        stream.extend(quote! {
            pub static #banner_pattern_ident: &BannerPattern = &BannerPattern {
                key: #key,
                asset_id: #asset_id,
                translation_key: #translation_key,
            };
        });
    }

    // Generate registration function
    let mut register_stream = TokenStream::new();
    for (banner_pattern_name, _) in &banner_patterns {
        let banner_pattern_ident = Ident::new(
            &banner_pattern_name.to_shouty_snake_case(),
            Span::call_site(),
        );
        register_stream.extend(quote! {
            registry.register(#banner_pattern_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_banner_patterns(registry: &mut BannerPatternRegistry) {
            #register_stream
        }
    });

    stream
}
