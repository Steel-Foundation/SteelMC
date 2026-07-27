use crate::generator_functions::{
    generate_identifier, read_json_asset, sort_contiguous_registry_entries,
};
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize)]
struct CustomStatEntry {
    id: usize,
    key: Identifier,
}

pub(crate) fn build() -> TokenStream {
    const ASSET: &str = "build_assets/custom_stats.json";

    let mut custom_stats: Vec<CustomStatEntry> = read_json_asset(ASSET);
    sort_contiguous_registry_entries(&mut custom_stats, ASSET, |entry| entry.id);

    let mut constants = TokenStream::new();
    let mut registrations = TokenStream::new();

    for custom_stat in &custom_stats {
        let ident = Ident::new(
            &custom_stat.key.path.to_shouty_snake_case(),
            Span::call_site(),
        );
        let key = generate_identifier(&custom_stat.key);

        constants.extend(quote! {
            pub static #ident: CustomStat = CustomStat {
                key: #key,
            };
        });

        registrations.extend(quote! {
            registry.register(&#ident);
        });
    }

    quote! {
        use crate::stat::custom::{CustomStat, CustomStatRegistry};
        use std::borrow::Cow;
        use steel_utils::Identifier;

        #constants

        pub fn register_custom_stats(registry: &mut CustomStatRegistry) {
            #registrations
        }
    }
}
