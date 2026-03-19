use quote::quote;
use std::{collections::BTreeMap, fs};

use crate::to_block_ident;

pub fn build() -> String {
    println!("cargo:rerun-if-changed=build/waxables.json");

    let waxables_json =
        fs::read_to_string("build/waxables.json").expect("Failed to read waxables.json");
    let waxables_raw: BTreeMap<String, String> =
        serde_json::from_str(&waxables_json).expect("Failed to parse waxables.json");

    let waxables: Vec<proc_macro2::TokenStream> = waxables_raw
            .iter()
            .map(|(normal, stripped)| (to_block_ident(normal), to_block_ident(stripped)))
            .map(|(from, to)| quote! { b if ptr::eq(b, vanilla_blocks::#from) => Some(vanilla_blocks::#to) , })
            .collect();

    let waxables_reverse: Vec<proc_macro2::TokenStream> = waxables_raw
            .iter()
            .map(|(normal, stripped)| (to_block_ident(normal), to_block_ident(stripped)))
            .map(|(from, to)| quote! { b if ptr::eq(b, vanilla_blocks::#to) => Some(vanilla_blocks::#from) , })
            .collect();

    let output = quote! {
        //! Generated Mapping of Blocks made of Copper and their Waxed Variants

        use std::ptr;
        use steel_registry::vanilla_blocks;
        use steel_registry::blocks::BlockRef;

        /// Returns the `BlockRef` to the waxed Variant of Blocks made of Copper
        #[must_use]
        #[inline]
        pub fn get_waxed_from_normal_variant(block: BlockRef) -> Option<BlockRef> {
            match block {
                #(#waxables)*
                _ => None
            }
        }

        /// Returns the `BlockRef` unwaxed Variant of waxed Blocks made of Copper
        #[must_use]
        #[inline]
        pub fn get_normal_from_waxed_variant(block: BlockRef) -> Option<BlockRef> {
            match block {
                #(#waxables_reverse)*
                _ => None
            }
        }
    };

    output.to_string()
}
