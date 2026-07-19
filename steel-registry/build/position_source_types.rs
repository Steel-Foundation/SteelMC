use crate::generator_functions::{
    generate_identifier, read_json_asset, sort_contiguous_registry_entries,
};
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize)]
struct PositionSourceTypeEntry {
    id: usize,
    key: Identifier,
}

pub(crate) fn build() -> TokenStream {
    const ASSET: &str = "build_assets/position_source_types.json";

    let mut source_types: Vec<PositionSourceTypeEntry> = read_json_asset(ASSET);
    sort_contiguous_registry_entries(&mut source_types, ASSET, |entry| entry.id);

    let mut constants = TokenStream::new();
    let mut registrations = TokenStream::new();

    for source_type in &source_types {
        let ident = Ident::new(
            &source_type.key.path.to_shouty_snake_case(),
            Span::call_site(),
        );
        let key = generate_identifier(&source_type.key);
        let payload_type = match source_type.key.path.as_ref() {
            "block" => quote! { BlockPositionSource },
            "entity" => quote! { EntityPositionSource },
            unknown => panic!("Unsupported extracted position source type: {unknown}"),
        };

        constants.extend(quote! {
            pub static #ident: PositionSourceType =
                PositionSourceType::of::<#payload_type>(#key);
        });

        registrations.extend(quote! {
            registry.register(&#ident);
        });
    }

    quote! {
        use crate::position_source::{
            BlockPositionSource, EntityPositionSource, PositionSourceType,
            PositionSourceTypeRegistry,
        };
        use std::borrow::Cow;
        use steel_utils::Identifier;

        #constants

        pub fn register_position_source_types(registry: &mut PositionSourceTypeRegistry) {
            #registrations
        }
    }
}
