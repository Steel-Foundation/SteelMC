//! Code generation for item behaviors.
//!
//! Scans `src/behavior/items/**/*.rs` for structs annotated with `#[item_behavior]`,
//! cross-references with `classes.json`, and generates `register_item_behaviors()`.

use crate::common::{self, JsonArgKind, scan_object_behaviors};
use proc_macro2::{Ident, Span};
use quote::quote;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize)]
pub struct ItemClass {
    pub name: String,
    pub class: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// --- Code generation ---

pub fn build(items: &[ItemClass]) -> String {
    let discovered = scan_object_behaviors("items", "item_behavior");

    let mut type_imports = BTreeSet::new();
    let mut enum_imports: BTreeMap<String, String> = BTreeMap::new();
    let mut registry_modules_used: BTreeSet<String> = BTreeSet::new();
    let mut registrations = Vec::new();
    let mut matched_classes = BTreeSet::new();

    for item in items {
        let Some(info) = discovered.get(&item.class) else {
            continue;
        };

        matched_classes.insert(&item.class);

        let struct_ident = Ident::new(&info.struct_name, Span::call_site());
        let item_field = Ident::new(&item.name, Span::call_site());

        type_imports.insert(info.struct_name.clone());

        for field in &info.fields {
            match &field.kind {
                JsonArgKind::Enum {
                    type_name,
                    module_path,
                } => {
                    if let Some(path) = module_path {
                        enum_imports.insert(type_name.clone(), path.clone());
                    } else {
                        type_imports.insert(type_name.clone());
                    }
                }
                JsonArgKind::Registry(module) => {
                    registry_modules_used.insert(module.clone());
                }
                JsonArgKind::Value => {}
            }
        }

        // Need to divide here into two cases because blocks always have a block property while items don't have that.
        let registration = if info.fields.is_empty() {
            // Unit struct or struct with no json_args — instantiate directly
            quote! {
                registry.set_behavior(
                    &vanilla_items::ITEMS.#item_field,
                    Box::new(#struct_ident),
                );
            }
        } else {
            let mut args = Vec::new();
            for field in &info.fields {
                args.push(common::generate_arg(field, &item.extra, &item.name));
            }

            quote! {
                registry.set_behavior(
                    &vanilla_items::ITEMS.#item_field,
                    Box::new(#struct_ident::new(#(#args),*)),
                );
            }
        };

        registrations.push(registration);
    }

    for (class_name, info) in &discovered {
        assert!(
            matched_classes.contains(class_name),
            "Item behavior struct `{}` maps to class '{}' which doesn't exist in classes.json",
            info.struct_name,
            class_name
        );
    }

    // Build imports
    let item_type_imports: Vec<_> = type_imports
        .iter()
        .map(|name| Ident::new(name, Span::call_site()))
        .collect();

    let enum_import_tokens: Vec<_> = enum_imports
        .iter()
        .map(|(type_name, module_path)| {
            let type_ident = Ident::new(type_name, Span::call_site());
            let path: syn::Path = syn::parse_str(module_path).unwrap_or_else(|_| {
                panic!("Invalid module path '{module_path}' for enum '{type_name}'")
            });
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
        //! Generated item behavior assignments.

        use steel_registry::{vanilla_items #(#registry_import_tokens)*};
        use crate::behavior::ItemBehaviorRegistry;
        use crate::behavior::items::{#(#item_type_imports),*};
        #(#enum_import_tokens)*

        pub fn register_item_behaviors(registry: &mut ItemBehaviorRegistry) {
            #(#registrations)*
        }
    };

    output.to_string()
}
