use std::{collections::BTreeMap, fs};

use quote::quote;

use crate::to_block_ident;

pub fn build() -> String {
    println!("cargo:rerun-if-changed=build/strippables.json");

    let strippables_json =
        fs::read_to_string("build/strippables.json").expect("Failed to read strippables.json");
    let strippables_entries: BTreeMap<String, String> =
        serde_json::from_str(&strippables_json).expect("Failed to parse strippables.json");

    let strippables: Vec<proc_macro2::TokenStream> = strippables_entries
        .iter()
        .map(|(normal, stripped)| (to_block_ident(normal), to_block_ident(stripped)))
        .map(|(from, to)| quote! { b if b == vanilla_blocks::#from => Some(vanilla_blocks::#to) , })
        .collect();

    let output = quote! {
        //! Generated Mapping of Logs and Woods to their stripped Variant

        use steel_registry::vanilla_blocks;
        use steel_registry::blocks::BlockRef;

        /// Returns the `BlockRef` to the stripped Variant of Logs and Woods
        #[must_use]
        #[inline]
        pub fn get_strippable_variant(block: BlockRef) -> Option<BlockRef> {
            match block {
                #(#strippables)*
                _ => None
            }
        }
    };

    output.to_string()
}
