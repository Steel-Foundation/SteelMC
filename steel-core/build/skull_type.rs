use std::{collections::BTreeMap, fs};

use quote::quote;
use serde::Deserialize;

use crate::to_block_ident;

#[derive(Deserialize)]
struct SkullTypeEntry {
    wall_block: String,
    block: String,
}

pub fn build() -> String {
    println!("cargo:rerun-if-changed=build/skull_types.json");

    let skull_types_json =
        fs::read_to_string("build/skull_types.json").expect("Failed to read skull_types.json");
    let skull_types_raw: BTreeMap<String, SkullTypeEntry> =
        serde_json::from_str(&skull_types_json).expect("Failed to parse skull_types.json");

    let skull_types: Vec<proc_macro2::TokenStream> = skull_types_raw
        .iter()
        .map(|(key, value)| {
            let ident = to_block_ident(key);
            let block = to_block_ident(&value.block);
            let wall_block = to_block_ident(&value.wall_block);
            quote! { pub static #ident: SkullType = SkullType {
                key: Identifier::vanilla_static(#key),
                block: &vanilla_blocks::#block,
                wall_block: &vanilla_blocks::#wall_block,
            }; }
        })
        .collect();

    let all_idents = skull_types_raw.keys().map(|key| to_block_ident(key));

    let output = quote! {
        use steel_registry::{blocks::BlockRef, vanilla_blocks};
        use steel_utils::Identifier;
        use crate::behavior::blocks::{SkullType, SkullTypeRef};

        #(#skull_types)*

        pub static ALL: &[SkullTypeRef] = &[#(&#all_idents),*];
    };

    output.to_string()
}
