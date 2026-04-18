use rustc_hash::FxHashMap;
use std::{fs, path::Path};

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

use super::tag_utils;

#[derive(Deserialize)]
struct TagFile {
    block: FxHashMap<String, Vec<String>>,
}

fn read_all_fabric_tags(tag_file: &str) -> FxHashMap<String, Vec<String>> {
    if fs::exists(tag_file).unwrap_or(false)
        && Path::new(tag_file).is_file()
        && let Ok(content) = fs::read_to_string(tag_file)
    {
        let tag: TagFile = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {}", tag_file, e));
        return tag.block;
    }
    FxHashMap::default()
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/tags/block/");

    let tag_dir = "build_assets/builtin_datapacks/minecraft/tags/block";
    let mut all_tags = tag_utils::read_all_tags(tag_dir);
    all_tags.extend(read_all_fabric_tags("build_assets/tags.json"));

    let sorted_tags = tag_utils::resolve_all_tags(&all_tags);

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::blocks::BlockRegistry;
        use crate::TaggedRegistryExt;
        use steel_utils::Identifier;
    });

    let mut register_stream = TokenStream::new();
    let mut static_array = TokenStream::new();
    let mut const_identifier = TokenStream::new();
    for (tag_name, blocks) in &sorted_tags {
        let tag_ident_array = Ident::new(
            &format!("{}_TAG_LIST", tag_name.to_shouty_snake_case()),
            Span::call_site(),
        );

        let block_strs = blocks.iter().map(|s| s.as_str());

        static_array.extend(quote! {
            static #tag_ident_array: &[&str] = &[#(#block_strs),*];
        });
        let tag_ident = Ident::new(
            &format!("{}_TAG", tag_name.to_shouty_snake_case()),
            Span::call_site(),
        );
        let tag_key = tag_name.clone();

        if let Some(key) = tag_key.strip_prefix("c:") {
            const_identifier.extend(
                quote! { pub const #tag_ident: Identifier = Identifier::new_static("c", #key); },
            );
        } else {
            const_identifier.extend(
                quote! {pub const #tag_ident: Identifier = Identifier::vanilla_static(#tag_key);},
            );
        }
        register_stream.extend(quote! {
            registry.register_tag(
                #tag_ident,
                #tag_ident_array
            );
        });
    }
    stream.extend(quote! {
        #static_array

        #const_identifier
        pub fn register_block_tags(registry: &mut BlockRegistry) {
            #register_stream
        }
    });

    stream
}
