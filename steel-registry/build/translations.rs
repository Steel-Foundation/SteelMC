use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde_json::Value;

/// Count the number of parameters in a translation string
fn count_parameters(text: &str) -> usize {
    let sequential = text.matches("%s").count();
    let mut positional = 0;
    for i in 1..=20 {
        if text.contains(&format!("%{}$s", i)) {
            positional = positional.max(i);
        }
    }
    sequential.max(positional)
}

/// Convert a translation key to a valid Rust constant name
/// Preserves the distinction between dots and camelCase
fn key_to_const_name(key: &str) -> String {
    // Split by dots and convert each segment separately to preserve camelCase
    let segments: Vec<String> = key
        .split('.')
        .map(|segment| {
            // Convert segment to SHOUTY_SNAKE_CASE
            segment.to_shouty_snake_case()
        })
        .collect();

    // Join with double underscores to distinguish from camelCase-generated underscores
    segments.join("_").replace('-', "_")
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
        use steel_utils::text::translation::{Translation, Args0, Args1, Args2, Args3, Args4, Args5, Args6, Args7, Args8};
    });

    // Generate constants for each translation
    let mut translations_vec: Vec<_> = translations.iter().collect();
    translations_vec.sort_by_key(|(k, _)| *k);

    // Track used constant names to handle collisions
    let mut used_names = rustc_hash::FxHashMap::default();

    for (key, value) in translations_vec {
        let text = match value.as_str() {
            Some(t) => t,
            None => {
                eprintln!(
                    "Warning: Translation key '{}' has non-string value, skipping",
                    key
                );
                continue;
            }
        };

        let param_count = count_parameters(text);

        // Skip translations with more than 8 parameters
        if param_count > 8 {
            eprintln!(
                "Warning: Translation '{}' has {} parameters (max 8 supported), skipping",
                key, param_count
            );
            continue;
        }

        let mut const_name_str = key_to_const_name(key);

        // Handle collisions by appending a number
        if let Some(count) = used_names.get_mut(&const_name_str) {
            *count += 1;
            const_name_str = format!("{}_{}", const_name_str, count);
        } else {
            used_names.insert(const_name_str.clone(), 1);
        }

        let const_name = Ident::new(&const_name_str, Span::call_site());
        let escaped_text = escape_string(text);

        let args_type = match param_count {
            0 => quote! { Args0 },
            1 => quote! { Args1 },
            2 => quote! { Args2 },
            3 => quote! { Args3 },
            4 => quote! { Args4 },
            5 => quote! { Args5 },
            6 => quote! { Args6 },
            7 => quote! { Args7 },
            8 => quote! { Args8 },
            _ => unreachable!(),
        };

        stream.extend(quote! {
            pub static #const_name: Translation<#args_type> = Translation::new(
                #key,
                #escaped_text
            );
        });
    }

    stream
}
