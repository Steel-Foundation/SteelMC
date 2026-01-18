use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Deserialize)]
struct EntityTypeEntry {
    id: i32,
    name: String,
    client_tracking_range: i32,
    update_interval: i32,
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/entities.json");

    let entities_file = "build_assets/entities.json";
    let content = fs::read_to_string(entities_file).unwrap();
    let entity_types: Vec<EntityTypeEntry> = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse entities.json: {}", e));

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::entity_types::{EntityType, EntityTypeRegistry};
    });

    for entity_type in &entity_types {
        let entity_type_ident =
            Ident::new(&entity_type.name.to_shouty_snake_case(), Span::call_site());
        let entity_type_key = &entity_type.name;
        let id = entity_type.id;
        let client_tracking_range = entity_type.client_tracking_range;
        let update_interval = entity_type.update_interval;

        stream.extend(quote! {
            pub static #entity_type_ident: &EntityType = &EntityType {
                key: #entity_type_key,
                id: #id,
                client_tracking_range: #client_tracking_range,
                update_interval: #update_interval,
            };
        });
    }

    let mut register_stream = TokenStream::new();
    for entity_type in &entity_types {
        let entity_type_ident =
            Ident::new(&entity_type.name.to_shouty_snake_case(), Span::call_site());
        register_stream.extend(quote! {
            registry.register(#entity_type_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_entity_types(registry: &mut EntityTypeRegistry) {
            #register_stream
        }
    });

    stream
}
