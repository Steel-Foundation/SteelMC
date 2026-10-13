use crate::generator_functions::{
    generate_identifier, read_json_asset, sort_contiguous_registry_entries,
};
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize)]
struct ActivityEntry {
    id: usize,
    key: Identifier,
}

pub(crate) fn build() -> TokenStream {
    const ASSET: &str = "build_assets/activities.json";

    let mut activities: Vec<ActivityEntry> = read_json_asset(ASSET);
    sort_contiguous_registry_entries(&mut activities, ASSET, |entry| entry.id);

    let mut constants = TokenStream::new();
    let mut registrations = TokenStream::new();

    for activity in &activities {
        let ident = Ident::new(&activity.key.path.to_shouty_snake_case(), Span::call_site());
        let key = generate_identifier(&activity.key);

        constants.extend(quote! {
            pub static #ident: Activity = Activity { key: #key };
        });

        registrations.extend(quote! {
            registry.register(&#ident);
        });
    }

    quote! {
        use crate::activity::{Activity, ActivityRegistry};
        use std::borrow::Cow;
        use steel_utils::Identifier;

        #constants

        pub fn register_activities(registry: &mut ActivityRegistry) {
            #registrations
        }
    }
}
