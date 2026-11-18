use std::{collections::BTreeMap, fs};

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
/// The generated module exposes the vanilla group names, one collection identifier
/// constant per level, one code-owned collection per level, and a helper that builds
/// the default vanilla groups referencing those collections. Collections hold the
/// level's own commands; cumulative behaviour comes from the group inheritance chain.
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

    let group_constants = group_name_constants(&group_names);
    let collection_constants = collection_identifier_constants(&group_names);
    let collection_entries = collection_entries(&group_names, &groups);
    let default_group_entries = default_group_entries(&group_names, &groups);

    let output = quote! {
        //! Generated vanilla permission groups and collections. Do not edit by hand.

        use std::collections::BTreeMap;

        use steel_utils::Identifier;

        use crate::permission::PermissionGroupConfig;

        #group_constants

        #collection_constants

        /// Vanilla permission level applied to every player by default.
        pub static DEFAULT_GROUP: &str = #default;
        /// Vanilla permission level assigned by `/op`.
        pub static OP_GROUP: &str = #op;
        /// Vanilla permission levels granted to command blocks.
        pub static COMMAND_BLOCK_GROUPS: &[&str] = &[#command_block];
        /// Vanilla permission level names.
        pub static GROUPS: &[&str] = &[#(#group_names),*];

        /// Returns the code-owned vanilla collections, keyed by `minecraft:<level>`.
        ///
        /// Each collection holds the commands declared for that level only. Levels
        /// inherit cumulatively through the generated default groups.
        pub fn collections() -> Vec<(Identifier, Vec<Identifier>)> {
            vec![
                #collection_entries
            ]
        }

        /// Returns the default vanilla groups referencing the generated collections.
        pub fn default_groups() -> BTreeMap<String, PermissionGroupConfig> {
            let mut groups = BTreeMap::new();
            #default_group_entries
            groups
        }
    };
    output.to_string()
}

/// Emits the `&str` constants holding each vanilla permission level name.
fn group_name_constants(group_names: &[String]) -> TokenStream {
    let mut tokens = TokenStream::new();
    for name in group_names {
        let constant = level_constant(name);
        tokens.extend(quote! {
            #[doc = #name]
            #[doc = "permission level group name."]
            pub static #constant: &str = #name;
        });
    }
    tokens
}

/// Emits the `Identifier` constants for each vanilla permission collection.
fn collection_identifier_constants(group_names: &[String]) -> TokenStream {
    let mut tokens = TokenStream::new();
    for name in group_names {
        let collection = collection_constant(name);
        tokens.extend(quote! {
            #[doc = #name]
            #[doc = "vanilla permission collection identifier."]
            pub const #collection: Identifier = Identifier::new_static("minecraft", #name);
        });
    }
    tokens
}

/// Emits the `(Identifier, Vec<Identifier>)` entries for the code-owned collections.
fn collection_entries(group_names: &[String], groups: &BTreeMap<String, Group>) -> TokenStream {
    let mut tokens = TokenStream::new();
    for name in group_names {
        let collection = collection_constant(name);
        let permissions = groups.get(name).map_or_else(Vec::new, |group| {
            group
                .allow
                .iter()
                .map(|key| permission_identifier(key))
                .collect()
        });
        let permissions = permissions
            .iter()
            .map(|(namespace, path)| quote! { Identifier::new_static(#namespace, #path) })
            .collect::<Vec<_>>();
        tokens.extend(quote! {
            (
                #collection,
                vec![#(#permissions),*],
            ),
        });
    }
    tokens
}

/// Emits the default vanilla group configs referencing the generated collections.
fn default_group_entries(group_names: &[String], groups: &BTreeMap<String, Group>) -> TokenStream {
    let mut tokens = TokenStream::new();
    for name in group_names {
        let inherits = groups
            .get(name)
            .and_then(|group| group.inherits.clone())
            .into_iter()
            .collect::<Vec<_>>();
        let has_allow = groups
            .get(name)
            .is_some_and(|group| !group.allow.is_empty());
        let allow = has_allow.then(|| format!("#minecraft:{name}")).into_iter();
        tokens.extend(quote! {
            groups.insert(
                #name.to_owned(),
                PermissionGroupConfig {
                    inherits: vec![#(#inherits.to_owned()),*],
                    allow: vec![#(#allow.to_owned()),*],
                    ..PermissionGroupConfig::default()
                },
            );
        });
    }
    tokens
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

fn collection_constant(name: &str) -> Ident {
    Ident::new(
        &format!("{}_COLLECTION", name.to_shouty_snake_case()),
        Span::call_site(),
    )
}

/// Splits a dotted extracted permission key into its namespace and path.
fn permission_identifier(key: &str) -> (String, String) {
    match key.split_once('.') {
        Some((namespace, path)) => (namespace.to_owned(), path.to_owned()),
        None => ("minecraft".to_owned(), key.to_owned()),
    }
}
