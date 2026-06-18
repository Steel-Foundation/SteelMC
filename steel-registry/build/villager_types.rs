use crate::generator_functions::{
    generate_identifier, read_json_asset, sort_contiguous_registry_entries,
};
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize)]
struct VillagerTypeEntry {
    id: usize,
    key: Identifier,
}

pub(crate) fn build() -> TokenStream {
    const ASSET: &str = "build_assets/villager_types.json";

    let mut villager_types: Vec<VillagerTypeEntry> = read_json_asset(ASSET);
    sort_contiguous_registry_entries(&mut villager_types, ASSET, |entry| entry.id);

    let mut constants = TokenStream::new();
    let mut registrations = TokenStream::new();

    for villager_type in &villager_types {
        let ident = Ident::new(
            &villager_type.key.path.to_shouty_snake_case(),
            Span::call_site(),
        );
        let key = generate_identifier(&villager_type.key);

        constants.extend(quote! {
            pub static #ident: VillagerType = VillagerType {
                key: #key,
            };
        });

        registrations.extend(quote! {
            registry.register(&#ident);
        });
    }

    quote! {
        use crate::villager_type::{VillagerType, VillagerTypeRegistry};
        use std::borrow::Cow;
        use steel_utils::Identifier;

        #constants

        pub fn register_villager_types(registry: &mut VillagerTypeRegistry) {
            #registrations
        }
    }
}
