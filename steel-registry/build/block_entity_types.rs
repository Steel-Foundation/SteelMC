#![expect(
    clippy::unwrap_used,
    reason = "build script must fail immediately on invalid extracted block entity type data"
)]

use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Deserialize)]
struct BlockEntityTypesJson {
    block_entity_types: Vec<BlockEntityTypeJson>,
}

#[derive(Deserialize)]
struct BlockEntityTypeJson {
    name: String,
    valid_blocks: Vec<String>,
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/block_entities.json");

    let block_entity_types_file = "build_assets/block_entities.json";
    let content = fs::read_to_string(block_entity_types_file).unwrap();
    let block_entity_types: BlockEntityTypesJson = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse block_entities.json: {e}"));

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::block_entity_type::{
            BlockEntityType, BlockEntityTypeRegistry,
        };
        use crate::vanilla_blocks;
        use steel_utils::Identifier;
    });

    // Generate static block entity type definitions
    let mut register_stream = TokenStream::new();
    for block_entity_type in &block_entity_types.block_entity_types {
        let block_entity_type_name = &block_entity_type.name;
        let block_entity_type_ident = Ident::new(
            &block_entity_type_name.to_shouty_snake_case(),
            Span::call_site(),
        );
        let block_entity_type_name_str = block_entity_type_name.clone();
        let valid_block_idents = block_entity_type
            .valid_blocks
            .iter()
            .map(|block_name| Ident::new(&block_name.to_shouty_snake_case(), Span::call_site()));

        let key = quote! { Identifier::vanilla_static(#block_entity_type_name_str) };

        stream.extend(quote! {
            pub static #block_entity_type_ident: BlockEntityType = BlockEntityType {
                key: #key,
                valid_blocks: &[#(&vanilla_blocks::#valid_block_idents),*],
            };
        });
        register_stream.extend(quote! {
            registry.register(&#block_entity_type_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_block_entity_types(registry: &mut BlockEntityTypeRegistry) {
            #register_stream
        }
    });

    stream
}
