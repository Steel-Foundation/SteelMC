use std::{collections::BTreeMap, fs};

use quote::quote;

use crate::items::to_block_const;

pub fn build() -> String {
    println!("cargo:rerun-if-changed=build/weathering.json");

    let oxidizables_json =
        fs::read_to_string("build/weathering.json").expect("Failed to read weathering.json");
    let oxidizables_raw: BTreeMap<String, String> =
        serde_json::from_str(&oxidizables_json).expect("Failed to parse weathering.json");

    let oxidizables: Vec<proc_macro2::TokenStream> = oxidizables_raw
        .iter()
        .map(|(current, next)| (to_block_const(current), to_block_const(next)))
        .map(|(from, to)| quote! { b if ptr::eq(b, vanilla_blocks::#from) => Some(vanilla_blocks::#to) , })
        .collect();

    let oxidizables_reverse: Vec<proc_macro2::TokenStream> = oxidizables_raw
        .iter()
        .map(|(current, next)| (to_block_const(current), to_block_const(next)))
        .map(|(from, to)| quote! { b if ptr::eq(b, vanilla_blocks::#to) => Some(vanilla_blocks::#from) , })
        .collect();

    let output = quote! {
        use std::ptr;
        use steel_registry::{blocks::BlockRef, vanilla_blocks};

        #[must_use]
        #[inline]
        pub fn next_copper_stage(block: BlockRef) -> Option<BlockRef> {
            match block {
                #(#oxidizables)*
                _ => None
            }
        }

        #[must_use]
        #[inline]
        pub fn previous_copper_stage(block: BlockRef) -> Option<BlockRef> {
            match block {
                #(#oxidizables_reverse)*
                _ => None
            }
        }
    };

    output.to_string()
}
