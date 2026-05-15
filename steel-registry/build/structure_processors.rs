//! Build-time codegen for vanilla structure processor-list registry entries.

use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

#[allow(dead_code)]
#[path = "../src/structure_processor/data.rs"]
mod structure_processor_data;

use structure_processor_data::StructureProcessorListData;

fn sorted_json_files(dir: &str) -> Vec<fs::DirEntry> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("{dir} missing: {err}"))
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    files.sort_by_key(|entry| entry.file_name());
    files
}

fn resource_name(entry: &fs::DirEntry) -> String {
    entry
        .path()
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_else(|| {
            panic!(
                "invalid structure processor-list file name: {:?}",
                entry.path()
            )
        })
        .to_owned()
}

pub(crate) fn build() -> TokenStream {
    let dir = "build_assets/builtin_datapacks/minecraft/worldgen/processor_list";
    println!("cargo:rerun-if-changed={dir}");

    let mut entries = Vec::new();
    for entry in sorted_json_files(dir) {
        let name = resource_name(&entry);
        let path = entry.path();
        let content =
            fs::read_to_string(&path).unwrap_or_else(|err| panic!("failed to read {name}: {err}"));
        serde_json::from_str::<StructureProcessorListData>(&content)
            .unwrap_or_else(|err| panic!("failed to parse structure processor list {name}: {err}"));
        entries.push((name, path.to_string_lossy().replace('\\', "/")));
    }

    let mut stream = TokenStream::new();
    stream.extend(quote! {
        use crate::structure_processor::{
            StructureProcessorList, StructureProcessorListData, StructureProcessorListRegistry,
        };
        use steel_utils::Identifier;
        use std::sync::{LazyLock, OnceLock};
    });

    let mut register = TokenStream::new();
    for (name, path) in &entries {
        let ident = Ident::new(&name.to_shouty_snake_case(), Span::call_site());
        let include_path = format!("../../{path}");
        stream.extend(quote! {
            pub static #ident: LazyLock<StructureProcessorList> = LazyLock::new(|| {
                let data = match serde_json::from_str::<StructureProcessorListData>(include_str!(#include_path)) {
                    Ok(data) => data,
                    Err(err) => panic!("failed to parse generated structure processor list {}: {err}", #name),
                };
                StructureProcessorList {
                    key: Identifier::vanilla_static(#name),
                    data,
                    id: OnceLock::new(),
                }
            });
        });
        register.extend(quote! {
            registry.register(&#ident);
        });
    }

    stream.extend(quote! {
        pub fn register_structure_processor_lists(registry: &mut StructureProcessorListRegistry) {
            #register
        }
    });

    stream
}
