//! Code generation for block behaviors.
//!
//! Scans `src/behavior/blocks/**/*.rs` for structs annotated with `#[block_behavior]`,
//! cross-references with `classes.json`, and generates `register_block_behaviors()`.

use proc_macro2::{Ident, Span};
use quote::quote;
use serde::Deserialize;
use std::collections::BTreeSet;

use crate::{
    common::{self, GeneratedImports, scan_object_behaviors},
    to_block_ident,
};

#[derive(Debug, Deserialize)]
pub struct BlockClass {
    pub name: String,
    pub class: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

pub fn build(blocks: &[BlockClass]) -> String {
    let discovered = scan_object_behaviors("blocks", "block_behavior");

    let mut imports = GeneratedImports::default();
    let mut registrations = Vec::new();
    let mut matched_classes = BTreeSet::new();

    for block in blocks {
        let Some(info) = discovered.get(&block.class) else {
            continue;
        };
        matched_classes.insert(&block.class);

        let struct_ident = Ident::new(&info.struct_name, Span::call_site());
        let const_ident = to_block_ident(&block.name);

        imports.add_fields(&info.fields);

        let mut args = Vec::new();
        for field in &info.fields {
            args.push(common::generate_arg(field, &block.extra, &block.name));
        }

        let registration = quote! {
            registry.set_behavior(
                &vanilla_blocks::#const_ident,
                Box::new(#struct_ident::new(&vanilla_blocks::#const_ident #(, #args)*)),
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

    let enum_import_tokens = imports.enum_import_tokens();
    let registry_import_tokens = imports.registry_import_tokens("vanilla_blocks");

    let output = quote! {
        //! Generated block behavior assignments.

        use steel_registry::{vanilla_blocks #(#registry_import_tokens)*};
        use crate::behavior::BlockBehaviorRegistry;
        #[expect(
            clippy::wildcard_imports,
            reason = "the registry intentionally imports every block behavior implementation"
        )]
        use crate::behavior::blocks::*;
        #(#enum_import_tokens)*

        pub fn register_block_behaviors(registry: &mut BlockBehaviorRegistry) {
            #(#registrations)*
        }
    };

    output.to_string()
}
