//! Code generation for block behaviors.
//!
//! Scans `src/behavior/blocks/**/*.rs` for structs annotated with `#[block_behavior]`,
//! cross-references with `classes.json`, and generates `register_block_behaviors()`.

use proc_macro2::{Ident, Span};
use quote::quote;
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
use std::env;
use std::fs;

use crate::common::{self, JsonArgField, JsonArgKind};

#[derive(Debug, Deserialize)]
pub struct BlockClass {
    pub name: String,
    pub class: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// --- Source scanning ---

#[derive(Debug, Clone)]
struct DiscoveredBlock {
    struct_name: String,
    class_names: Vec<String>,
    fields: Vec<JsonArgField>,
}

fn extract_class_name(attr: &syn::Attribute) -> Vec<String> {
    let syn::Meta::List(meta) = &attr.meta else {
        return vec![];
    };

    let mut class_names = vec![];
    meta.parse_nested_meta(|meta| {
        if meta.path.is_ident("class") {
            let value = meta.value()?;
            let lit: syn::LitStr = value.parse()?;
            class_names.push(lit.value());
        }
        Ok(())
    })
    .unwrap_or_else(|e| panic!("Failed to parse block_behavior attribute: {e}"));
    class_names
}

fn parse_block_behavior(s: &syn::ItemStruct) -> Option<DiscoveredBlock> {
    let attr = s
        .attrs
        .iter()
        .find(|a| common::path_ends_with(a.path(), "block_behavior"))?;
    let mut class_names = extract_class_name(attr);
    if class_names.is_empty() {
        class_names.push(s.ident.to_string());
    }

    let mut fields = Vec::new();
    if let syn::Fields::Named(ref named) = s.fields {
        for field in &named.named {
            if let Some(json_arg) = common::parse_json_arg(field) {
                fields.push(json_arg);
            }
        }
    }

    Some(DiscoveredBlock {
        struct_name: s.ident.to_string(),
        class_names,
        fields,
    })
}

fn scan_block_behaviors() -> HashMap<String, DiscoveredBlock> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let pattern = format!("{manifest_dir}/src/behavior/blocks/**/*.rs");
    let mut discovered = HashMap::new();

    for entry in glob::glob(&pattern).expect("Failed to glob block behavior sources") {
        let path = entry.expect("Failed to read glob entry");
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
        let file = syn::parse_file(&content)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()));

        for item in &file.items {
            if let syn::Item::Struct(s) = item
                && let Some(block) = parse_block_behavior(s)
            {
                for class_name in &block.class_names {
                    discovered.insert(class_name.clone(), block.clone());
                }
            }
        }
    }

    discovered
}

// --- Code generation ---

pub fn build(blocks: &[BlockClass]) -> String {
    let discovered = scan_block_behaviors();

    let mut block_type_imports = BTreeSet::new();
    let mut registrations = Vec::new();
    let mut matched_classes = BTreeSet::new();

    for block in blocks {
        let Some(info) = discovered.get(&block.class) else {
            continue;
        };
        matched_classes.insert(&block.class);

        let struct_ident = Ident::new(&info.struct_name, Span::call_site());
        let const_ident = common::to_const_ident(&block.name);

        block_type_imports.insert(info.struct_name.clone());

        for field in &info.fields {
            if let JsonArgKind::Enum(ref enum_type) = field.kind {
                block_type_imports.insert(enum_type.clone());
            }
        }

        let mut args = Vec::new();
        for field in &info.fields {
            args.push(common::generate_arg(field, &block.extra, &block.name));
        }

        let registration = quote! {
            registry.set_behavior(
                vanilla_blocks::#const_ident,
                Box::new(#struct_ident::new(vanilla_blocks::#const_ident #(, #args)*)),
            );
        };

        registrations.push(registration);
    }

    // Verify all discovered structs matched a class in classes.json
    for (class_name, info) in &discovered {
        assert!(
            matched_classes.contains(class_name),
            "Block behavior struct `{}` maps to class '{}' which doesn't exist in classes.json",
            info.struct_name,
            class_name
        );
    }

    // Build imports
    let block_imports: Vec<_> = block_type_imports
        .iter()
        .map(|name| Ident::new(name, Span::call_site()))
        .collect();

    let output = quote! {
        //! Generated block behavior assignments.

        use steel_registry::{sound_events, vanilla_fluids, vanilla_blocks};
        use crate::behavior::BlockBehaviorRegistry;
        use crate::behavior::blocks::{#(#block_imports),*};

        pub fn register_block_behaviors(registry: &mut BlockBehaviorRegistry) {
            #(#registrations)*
        }
    };

    output.to_string()
}
