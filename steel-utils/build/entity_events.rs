use heck::ToUpperCamelCase;
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use std::fs;

#[derive(Deserialize)]
struct EntityEventJson {
    name: String,
    value: i32,
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/entity_events.json");

    let file = fs::read_to_string("build_assets/entity_events.json")
        .expect("Failed to read entity_events.json");

    let events: Vec<EntityEventJson> =
        serde_json::from_str(&file).expect("Failed to parse entity_events.json");

    let mut stream = TokenStream::new();

    for event in events {
        let variant_name = Ident::new(&event.name.to_upper_camel_case(), Span::call_site());
        let value = Literal::i32_unsuffixed(event.value);
        stream.extend(quote! {
            #variant_name = #value,
        });
    }

    quote! {
        use crate as steel_utils;
        use steel_macros::WriteTo;

        /// Status type for the entity event packet.
        #[derive(WriteTo, Clone, Copy, Debug, PartialEq, Eq)]
        #[write(as = VarInt)]
        pub enum EntityStatus {
            #stream
        }
    }
}
