use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde_json::Value;
use std::str::FromStr;

/// Count the number of parameters in a translation string
fn count_parameters(text: &str) -> usize {
    let sequential = text.matches("%s").count();
    let mut positional = 0;
    for i in 1..=20 {
        if text.contains(&format!("%{i}$s")) {
            positional = positional.max(i);
        }
    }
    sequential.max(positional)
}

/// Escape a string for use in Rust string literals
fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/en_us.json");

    let lang_file = fs::read_to_string("build_assets/en_us.json")
        .expect("Failed to read en_us.json language file");

    let translations: serde_json::Map<String, Value> =
        serde_json::from_str(&lang_file).expect("Failed to parse en_us.json");

    let mut stream = TokenStream::new();

    // Add imports
    stream.extend(quote! {
        use crate::text::translation::Translation;
        use phf;
    });

    // Generate constants for each translation
    let mut translations_vec: Vec<_> = translations.iter().collect();
    translations_vec.sort_by_key(|(k, _)| *k);

    // Track used constant names to handle collisions
    let mut used_names = rustc_hash::FxHashMap::default();

    // Store entries for the PHF map. The strings must live until `build()` is called on the map builder.
    let mut phf_map_entries = Vec::new();

    for (key, value) in translations_vec {
        let Some(text) = value.as_str() else {
            eprintln!("Warning: Translation key '{key}' has non-string value, skipping");
            continue;
        };

        let param_count = count_parameters(text);

        // Skip translations with more than 8 parameters
        if param_count > 8 {
            eprintln!(
                "Warning: Translation '{key}' has {param_count} parameters (max 8 supported), skipping"
            );
            continue;
        }

        let mut const_name_str = key.to_shouty_snake_case();

        // Handle collisions by appending a number
        if let Some(count) = used_names.get_mut(&const_name_str) {
            *count += 1;
            const_name_str = format!("{const_name_str}_{count}");
        } else {
            used_names.insert(const_name_str.clone(), 1);
        }

        let const_name = Ident::new(&const_name_str, Span::call_site());
        let escaped_text = escape_string(text);

        phf_map_entries.push((key.clone(), format!("\"{escaped_text}\"")));

        stream.extend(quote! {
            pub static #const_name: Translation<#param_count> = Translation::new(
                #key,
                #escaped_text
            );
        });
    }

    let mut map_builder = phf_codegen::Map::new();
    for (key, value) in &phf_map_entries {
        map_builder.entry(key, value);
    }

    let map_code = map_builder.build();
    let map_token_stream =
        TokenStream::from_str(&map_code.to_string()).expect("Unable to build token stream");

    stream.extend(quote! {
        pub static TRANSLATIONS: phf::Map<&'static str, &'static str> = #map_token_stream;
    });

    stream
}
