use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Default)]
struct Group {
    allow: Vec<String>,
    #[serde(default)]
    inherits: Option<String>,
}

/// Compiles the extracted vanilla permission levels into a Rust module.
///
/// The generated module exposes the vanilla group names, a cumulative allow-only
/// fallback per group, and a helper that inserts the vanilla groups as empty
/// configurable groups. Configured groups win over the fallback, which only fills
/// permissions left unset by configuration.
pub fn build() -> String {
    println!("cargo:rerun-if-changed=build/permissions_group.json");
    let json = fs::read_to_string("build/permissions_group.json")
        .expect("Failed to read permissions_group.json");
    let Value::Object(root) =
        serde_json::from_str(&json).expect("Failed to parse permissions_group.json")
    else {
        panic!("permissions_group.json must be a JSON object at the root")
    };
    let default = read_string(&root, "default");
    let op = read_string(&root, "op");
    let command_block = read_string(&root, "command_block");
    let groups = root
        .get("groups")
        .expect("permissions_group.json must contain a 'groups' key")
        .as_object()
        .expect("'groups' must be a JSON object")
        .iter()
        .map(|(name, value)| {
            let group: Group =
                serde_json::from_value(value.clone()).expect("Failed to parse permission group");
            (name.clone(), group)
        })
        .collect::<BTreeMap<_, _>>();
    let group_names = groups.keys().cloned().collect::<Vec<_>>();

    let mut group_constants = TokenStream::new();
    for name in &group_names {
        let constant = level_constant(name);
        group_constants.extend(quote! {
            #[doc = #name]
            #[doc = "permission level group name."]
            pub static #constant: &str = #name;
        });
    }

    let mut fallback_arms = TokenStream::new();
    for name in &group_names {
        let fallback = cumulative_fallback(name, &groups);
        fallback_arms.extend(quote! {
            #name => Some(&[#(#fallback),*]),
        });
    }

    let output = quote! {
        //! Generated vanilla permission groups. Do not edit by hand.

        use std::collections::BTreeMap;

        use crate::permission::PermissionGroupConfig;

        #group_constants

        /// Vanilla permission level applied to every player by default.
        pub static DEFAULT_GROUP: &str = #default;
        /// Vanilla permission level assigned by `/op`.
        pub static OP_GROUP: &str = #op;
        /// Vanilla permission levels granted to command blocks.
        pub static COMMAND_BLOCK_GROUPS: &[&str] = &[#command_block];
        /// Vanilla permission level names.
        pub static GROUPS: &[&str] = &[#(#group_names),*];

        /// Returns the cumulative vanilla command fallback for `group`.
        ///
        /// The fallback is allow-only and resolved through the vanilla inheritance
        /// chain, so a level already contains the commands of every level below it.
        pub fn fallback(group: &str) -> Option<&'static [&'static str]> {
            match group {
                #fallback_arms
                _ => None,
            }
        }

        /// Inserts the vanilla groups as empty configurable groups.
        ///
        /// Existing groups keep their configured edits. The vanilla defaults come
        /// from [`fallback`], not from these groups' own rules.
        pub fn insert_groups(groups: &mut BTreeMap<String, PermissionGroupConfig>) {
            for name in GROUPS {
                groups
                    .entry((*name).to_owned())
                    .or_insert_with(PermissionGroupConfig::default);
            }
        }
    };
    output.to_string()
}

fn read_string(root: &serde_json::Map<String, Value>, key: &str) -> String {
    root.get(key)
        .unwrap_or_else(|| panic!("permissions_group.json must contain a '{key}' key"))
        .as_str()
        .unwrap_or_else(|| panic!("'{key}' must be a string"))
        .to_owned()
}

fn level_constant(name: &str) -> Ident {
    Ident::new(
        &format!("{}_GROUP", name.to_shouty_snake_case()),
        Span::call_site(),
    )
}

fn cumulative_fallback(name: &str, groups: &BTreeMap<String, Group>) -> Vec<String> {
    let mut fallback = Vec::new();
    let mut visited = BTreeSet::new();
    collect_fallback(name, groups, &mut visited, &mut fallback);
    fallback
}

fn collect_fallback(
    name: &str,
    groups: &BTreeMap<String, Group>,
    visited: &mut BTreeSet<String>,
    fallback: &mut Vec<String>,
) {
    if !visited.insert(name.to_owned()) {
        return;
    }
    let Some(group) = groups.get(name) else {
        return;
    };
    if let Some(parent) = &group.inherits {
        collect_fallback(parent, groups, visited, fallback);
    }
    for rule in &group.allow {
        if !fallback.contains(rule) {
            fallback.push(rule.clone());
        }
    }
}
