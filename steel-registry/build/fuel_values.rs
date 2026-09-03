#![expect(
    clippy::unwrap_used,
    reason = "build script parses extracted vanilla data"
)]
use proc_macro2::{Ident, Literal, TokenStream};
use quote::quote;
use serde::Deserialize;
use std::fs;

#[derive(Deserialize)]
struct Fuel {
    item: String,
    burn_time: i32,
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/fuels.json");
    let fuels: Vec<Fuel> =
        serde_json::from_str(&fs::read_to_string("build_assets/fuels.json").unwrap()).unwrap();
    let arms = fuels.iter().map(|fuel| {
        let key = fuel.item.strip_prefix("minecraft:").unwrap_or(&fuel.item);
        let ident = Ident::new(
            &key.to_uppercase().replace('-', "_"),
            proc_macro2::Span::call_site(),
        );
        let burn = Literal::i32_suffixed(fuel.burn_time);
        quote! { _ if key == &vanilla_items::#ident.key => #burn, }
    });
    quote! {
        use crate::items::{ItemRef};
        use crate::vanilla_items;
        use steel_utils::Identifier;
        #[must_use]
        pub fn burn_duration(item: ItemRef) -> i32 {
            let key = &item.key;
            match key {
                #(#arms)*
                _ => 0,
            }
        }
    }
}
