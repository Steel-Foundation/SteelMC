use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Deserialize)]
struct AttributeEntry {
    id: u16,
    name: String,
    translation_key: String,
    default_value: f64,
    syncable: bool,
    min_value: f64,
    max_value: f64,
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/attributes.json");

    let content = fs::read_to_string("build_assets/attributes.json").unwrap();
    let mut attributes: Vec<AttributeEntry> = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse attributes.json: {}", e));

    // Sort by ID to guarantee registration order matches vanilla IDs
    attributes.sort_by_key(|a| a.id);

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::attribute::{Attribute, AttributeRegistry};
        use steel_utils::Identifier;
    });

    let mut register_stream = TokenStream::new();
    for attr in &attributes {
        let ident = Ident::new(&attr.name.to_shouty_snake_case(), Span::call_site());
        let name = &attr.name;
        let translation_key = &attr.translation_key;
        let default_value = Literal::f64_suffixed(attr.default_value);
        let min_value = Literal::f64_suffixed(attr.min_value);
        let max_value = Literal::f64_suffixed(attr.max_value);
        let syncable = attr.syncable;

        stream.extend(quote! {
            pub static #ident: &Attribute = &Attribute {
                key: Identifier::vanilla_static(#name),
                translation_key: #translation_key,
                default_value: #default_value,
                min_value: #min_value,
                max_value: #max_value,
                syncable: #syncable,
            };
        });

        register_stream.extend(quote! {
            registry.register(#ident);
        });
    }

    stream.extend(quote! {
        pub fn register_attributes(registry: &mut AttributeRegistry) {
            #register_stream
        }
    });

    stream
}
