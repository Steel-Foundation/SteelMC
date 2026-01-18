use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
pub struct TrimPatternJson {
    asset_id: Identifier,
    description: TextComponent,
    #[serde(default)]
    decal: bool,
}

#[derive(Deserialize, Debug)]
pub struct TextComponent {
    translate: String,
}

fn generate_identifier(resource: &Identifier) -> TokenStream {
    let namespace = resource.namespace.as_ref();
    let path = resource.path.as_ref();
    quote! { Identifier { namespace: Cow::Borrowed(#namespace), path: Cow::Borrowed(#path) } }
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/trim_pattern/"
    );

    let trim_pattern_dir = "build_assets/builtin_datapacks/minecraft/data/minecraft/trim_pattern";
    let mut trim_patterns = Vec::new();

    // Read all trim pattern JSON files
    for entry in fs::read_dir(trim_pattern_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let trim_pattern_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let trim_pattern: TrimPatternJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", trim_pattern_name, e));

            trim_patterns.push((trim_pattern_name, trim_pattern));
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::trim_pattern::{
            TrimPattern, TrimPatternRegistry,
        };
        use steel_utils::Identifier;
        use steel_utils::text::TextComponent;
        use std::borrow::Cow;
    });

    // Generate static trim pattern definitions
    for (trim_pattern_name, trim_pattern) in &trim_patterns {
        let trim_pattern_ident =
            Ident::new(&trim_pattern_name.to_shouty_snake_case(), Span::call_site());
        let trim_pattern_name_str = trim_pattern_name.clone();

        let key = quote! { Identifier::vanilla_static(#trim_pattern_name_str) };
        let asset_id = generate_identifier(&trim_pattern.asset_id);
        let translate = &trim_pattern.description.translate;
        let decal = trim_pattern.decal;

        stream.extend(quote! {
            pub static #trim_pattern_ident: &TrimPattern = &TrimPattern {
                key: #key,
                asset_id: #asset_id,
                description: TextComponent::const_translate(#translate),
                decal: #decal,
            };
        });
    }

    // Generate registration function
    let mut register_stream = TokenStream::new();
    for (trim_pattern_name, _) in &trim_patterns {
        let trim_pattern_ident =
            Ident::new(&trim_pattern_name.to_shouty_snake_case(), Span::call_site());
        register_stream.extend(quote! {
            registry.register(#trim_pattern_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_trim_patterns(registry: &mut TrimPatternRegistry) {
            #register_stream
        }
    });

    stream
}
