//! Code generation for block behaviors.
//!
//! Scans `src/behavior/blocks/**/*.rs` for structs annotated with `#[block_behavior]`,
//! cross-references with `classes.json`, and generates `register_block_behaviors()`.

use proc_macro2::{Ident, Span};
use quote::quote;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

use crate::common::{self, JsonArgKind, scan_object_behaviors};

#[derive(Debug, Deserialize)]
pub struct BlockClass {
    pub name: String,
    pub class: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// --- Code generation ---

pub fn build(blocks: &[BlockClass]) -> String {
    let discovered = scan_object_behaviors("blocks", "block_behavior");

    let mut block_type_imports = BTreeSet::new();
    let mut explicit_enum_imports: BTreeMap<String, String> = BTreeMap::new();
    let mut registry_modules_used: BTreeSet<String> = BTreeSet::new();
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
            match &field.kind {
                JsonArgKind::Enum {
                    type_name,
                    module_path,
                } => {
                    if let Some(path) = module_path {
                        explicit_enum_imports.insert(type_name.clone(), path.clone());
                    } else {
                        block_type_imports.insert(type_name.clone());
                    }
                }
                JsonArgKind::Registry(module) => {
                    registry_modules_used.insert(module.clone());
                }
                JsonArgKind::Value => {}
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

    let enum_import_tokens: Vec<_> = explicit_enum_imports
        .iter()
        .map(|(type_name, path)| {
            let type_ident = Ident::new(type_name, Span::call_site());
            let path: syn::Path = syn::parse_str(path)
                .unwrap_or_else(|_| panic!("Invalid module path '{path}' for enum '{type_name}'"));
            quote! { use #path::#type_ident; }
        })
        .collect();

    let registry_import_tokens: Vec<_> = registry_modules_used
        .iter()
        .map(|module| {
            let module_ident = Ident::new(module, Span::call_site());
            quote! { , #module_ident }
        })
        .collect();

    let output = quote! {
        //! Generated block behavior assignments.

        use steel_registry::{vanilla_blocks #(#registry_import_tokens)*};
        use crate::behavior::BlockBehaviorRegistry;
        use crate::behavior::blocks::{#(#block_imports),*};
        #(#enum_import_tokens)*

        pub fn register_block_behaviors(registry: &mut BlockBehaviorRegistry) {
            #(#registrations)*
        }
    };

    output.to_string()
}
