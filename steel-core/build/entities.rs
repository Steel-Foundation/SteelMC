//! Code generation for entity factories.
//!
//! Scans `src/entity/entities/**/*.rs` for structs annotated with `#[entity_behavior]`,
//! cross-references with `classes.json`, and generates `register_entity_factories()`.

use crate::common::{self, JsonArgKind, scan_object_behaviors_with_pattern};
use proc_macro2::{Ident, Span};
use quote::quote;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::env;

use crate::to_block_ident;

#[derive(Debug, Deserialize)]
pub struct EntityClass {
    pub name: String,
    pub class: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

pub fn build(entities: &[EntityClass]) -> String {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let pattern = format!("{manifest_dir}/src/entity/entities/**/*.rs");
    let discovered = scan_object_behaviors_with_pattern(&pattern, "entity_behavior");

    let mut type_imports = BTreeSet::new();
    let mut enum_imports: BTreeMap<String, String> = BTreeMap::new();
    let mut registry_modules_used: BTreeSet<String> = BTreeSet::new();
    let mut registrations = Vec::new();
    let mut matched_classes = BTreeSet::new();

    for entity in entities {
        let Some(info) = discovered.get(&entity.class) else {
            continue;
        };

        matched_classes.insert(&entity.class);

        let struct_ident = Ident::new(&info.struct_name, Span::call_site());
        let entity_type_ident = to_block_ident(&entity.name);

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

        let mut args = Vec::new();
        for field in &info.fields {
            args.push(common::generate_arg(field, &entity.extra, &entity.name));
        }

        let registration = quote! {
            registry.register(
                &vanilla_entities::#entity_type_ident,
                |entity_type, id, pos, world| {
                    let entity: SharedEntity =
                        Arc::new(#struct_ident::new(entity_type, id, pos, world #(, #args)*));
                    entity
                },
            );
            registry.register_load(
                &vanilla_entities::#entity_type_ident,
                |entity_type, load| {
                    let entity: SharedEntity =
                        Arc::new(#struct_ident::from_saved(entity_type, load #(, #args)*));
                    entity
                },
            );
        };

        registrations.push(registration);
    }

    for (class_name, info) in &discovered {
        assert!(
            matched_classes.contains(class_name),
            "Entity struct `{}` maps to class '{}' which doesn't exist in classes.json",
            info.struct_name,
            class_name
        );
    }

    let entity_type_imports: Vec<_> = type_imports
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
        .filter(|module| module.as_str() != "vanilla_entities")
        .map(|module| {
            let module_ident = Ident::new(module, Span::call_site());
            quote! { , #module_ident }
        })
        .collect();

    let output = quote! {
        //! Generated entity factory registrations.

        use std::sync::Arc;
        use steel_registry::{vanilla_entities #(#registry_import_tokens)*};
        use crate::entity::{EntityRegistry, SharedEntity};
        use crate::entity::entities::{#(#entity_type_imports),*};
        #(#enum_import_tokens)*

        pub fn register_entity_factories(registry: &mut EntityRegistry) {
            #(#registrations)*
        }
    };

    output.to_string()
}
