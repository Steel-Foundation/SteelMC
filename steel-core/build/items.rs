//! Code generation for item behaviors.
//!
//! Scans `src/behavior/items/**/*.rs` for structs annotated with `#[item_behavior]`,
//! cross-references with `classes.json`, and generates `register_item_behaviors()`.

use crate::common::{self, GeneratedImports, scan_object_behaviors};
use heck::ToShoutySnakeCase;
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

pub fn build(items: &[ItemClass]) -> String {
    let discovered = scan_object_behaviors("items", "item_behavior");

    let mut imports = GeneratedImports::default();
    let mut registrations = Vec::new();
    let mut matched_classes = BTreeSet::new();

    for item in items {
        let Some(info) = discovered.get(&item.class) else {
            continue;
        };

        matched_classes.insert(&item.class);

        let struct_ident = Ident::new(&info.struct_name, Span::call_site());
        let item_field = Ident::new(&item.name.to_shouty_snake_case(), Span::call_site());

        imports.add_fields(&info.fields);

        // Need to divide here into two cases because blocks always have a block property while items don't have that.
        let registration = if info.fields.is_empty() {
            // Unit struct or struct with no json_args — instantiate directly
            quote! {
                registry.set_behavior(
                    &*vanilla_items::#item_field,
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
                    &*vanilla_items::#item_field,
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

    let enum_import_tokens = imports.enum_import_tokens();
    let registry_import_tokens = imports.registry_import_tokens("vanilla_items");

    let output = quote! {
        //! Generated item behavior assignments.

        use steel_registry::{vanilla_items #(#registry_import_tokens)*};
        use crate::behavior::ItemBehaviorRegistry;
        #[expect(
            clippy::wildcard_imports,
            reason = "the registry intentionally imports every item behavior implementation"
        )]
        use crate::behavior::items::*;
        #(#enum_import_tokens)*

        pub fn register_item_behaviors(registry: &mut ItemBehaviorRegistry) {
            #(#registrations)*
        }
    };

    output.to_string()
}
