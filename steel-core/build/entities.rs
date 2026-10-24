//! Code generation for entity factories.
//!
//! Scans `src/entity/entities/**/*.rs` for structs annotated with `#[entity_behavior]`,
//! cross-references with `classes.json`, and generates `register_entity_factories()`
//! plus `register_spawn_rules()` from the `SPAWN_RULE` of structs that set `spawn_rule`.

use crate::common::{self, GeneratedImports, scan_object_behaviors_with_pattern};
use proc_macro2::{Ident, Span};
use quote::quote;
use serde::Deserialize;
use std::collections::BTreeSet;
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

    let mut imports = GeneratedImports::default();
    let mut registrations = Vec::new();
    let mut spawn_rules = Vec::new();
    let mut matched_classes = BTreeSet::new();

    for entity in entities {
        let Some(info) = discovered.get(&entity.class) else {
            continue;
        };

        matched_classes.insert(&entity.class);

        let struct_ident = Ident::new(&info.struct_name, Span::call_site());
        let entity_type_ident = to_block_ident(&entity.name);

        imports.add_fields(&info.fields);

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

        if info.spawn_rule {
            spawn_rules.push(quote! {
                registry.register_spawn_rule(
                    &vanilla_entities::#entity_type_ident,
                    #struct_ident::SPAWN_RULE,
                );
            });
        }
    }

    for (class_name, info) in &discovered {
        assert!(
            matched_classes.contains(class_name),
            "Entity struct `{}` maps to class '{}' which doesn't exist in classes.json",
            info.struct_name,
            class_name
        );
    }

    let enum_import_tokens = imports.enum_import_tokens();
    let registry_import_tokens = imports.registry_import_tokens("vanilla_entities");

    let output = quote! {
        //! Generated entity factory and spawn rule registrations.

        use std::sync::Arc;
        use steel_registry::{vanilla_entities #(#registry_import_tokens)*};
        use crate::entity::{EntityRegistry, SharedEntity};
        #[expect(
            clippy::wildcard_imports,
            reason = "the registry intentionally imports every entity implementation"
        )]
        use crate::entity::entities::*;
        #(#enum_import_tokens)*

        pub fn register_entity_factories(registry: &mut EntityRegistry) {
            #(#registrations)*
        }

        pub fn register_spawn_rules(registry: &mut EntityRegistry) {
            #(#spawn_rules)*
        }
    };

    output.to_string()
}
