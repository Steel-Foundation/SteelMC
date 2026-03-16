//! Code generation for item behaviors.
//!
//! Scans `src/behavior/items/**/*.rs` for structs annotated with `#[item_behavior]`,
//! cross-references with `classes.json`, and generates `register_item_behaviors()`.

use crate::common::{self, JsonArgKind, scan_object_behaviors};
use proc_macro2::{Ident, Span};
use quote::quote;
use serde::Deserialize;
use std::collections::BTreeSet;

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
    let mut enum_imports = BTreeSet::new();
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
            if let JsonArgKind::Enum(ref enum_type) = field.kind {
                enum_imports.insert(enum_type.clone());
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

    // Enum imports need to come from their actual module paths
    let enum_import_tokens: Vec<_> = enum_imports
        .iter()
        .map(|name| {
            // Direction lives in steel_registry::blocks::properties
            match name.as_str() {
                "Direction" => quote! { use steel_registry::blocks::properties::Direction; },
                _ => panic!(
                    "Unknown enum type '{name}' — add its import path to items.rs build script"
                ),
            }
        })
        .collect();

    let output = quote! {
        //! Generated item behavior assignments.

        use steel_registry::{vanilla_blocks, vanilla_items};
        use crate::behavior::ItemBehaviorRegistry;
        use crate::behavior::items::{#(#item_type_imports),*};
        #(#enum_import_tokens)*

        pub fn register_item_behaviors(registry: &mut ItemBehaviorRegistry) {
            #(#registrations)*
        }
    };

    output.to_string()
}
