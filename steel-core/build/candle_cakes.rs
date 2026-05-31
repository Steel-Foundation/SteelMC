use std::{collections::BTreeMap, fs};

use proc_macro2::Span;
use quote::quote;
use syn::Ident;

use crate::to_block_ident;

pub fn build() -> String {
    println!("cargo:rerun-if-changed=build/candle_cakes.json");

    let candle_cakes_json =
        fs::read_to_string("build/candle_cakes.json").expect("Failed to read candle_cakes.json");
    let candle_cakes_raw: BTreeMap<String, String> =
        serde_json::from_str(&candle_cakes_json).expect("Failed to parse candle_cakes.json");

    let by_candle: Vec<proc_macro2::TokenStream> = candle_cakes_raw
        .iter()
        .map(|(key, value)| (Ident::new(key.to_lowercase().as_str(), Span::call_site()), to_block_ident(value)))
        .map(|(key, value)| quote! { i if i == &vanilla_items::ITEMS.#key => Some(&vanilla_blocks::#value), })
        .collect();

    let output = quote! {
        use steel_registry::{blocks::BlockRef, items::ItemRef, vanilla_blocks, vanilla_items};

        pub fn candle_to_candle_cake(item: ItemRef) -> Option<BlockRef> {
            match item {
                #(#by_candle)*
                _ => None
            }
        }
    };

    output.to_string()
}
