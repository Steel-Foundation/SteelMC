use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;
use std::fs;
use std::str::FromStr;

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
        use phf;
    });

    // Generate constants for each translation
    let mut translations_vec: Vec<_> = translations.iter().collect();
    translations_vec.sort_by_key(|(k, _)| *k);

    // Store entries for the PHF map. The strings must live until `build()` is called on the map builder.
    let mut phf_map_entries = Vec::new();

    for (key, value) in translations_vec {
        let Some(text) = value.as_str() else {
            eprintln!("Warning: Translation key '{key}' has non-string value, skipping");
            continue;
        };
        let escaped_text = escape_string(text);

        phf_map_entries.push((key.clone(), format!("\"{escaped_text}\"")));
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
