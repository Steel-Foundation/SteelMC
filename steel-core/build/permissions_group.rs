use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeMap, fs, iter};

#[derive(Deserialize, Default)]
struct Group {
    allow: Vec<String>,
    #[serde(default)]
    inherits: Option<String>,
}

/// Vanilla level whose collection carries authority to manage the default group.
///
/// Change this to move that authority to a different level.
const DEFAULT_GROUP_MANAGER_LEVEL: &str = "admins";

/// Highest vanilla level, which carries the root management wildcards.
const OWNER_LEVEL: &str = "owners";

/// Compiles the extracted vanilla permission levels into a Rust module.
///
/// The generated module exposes the vanilla group names, one collection identifier
/// constant per level, one code-owned collection per level, and a helper that builds
/// the default vanilla groups referencing those collections. Collections hold the
/// level's own commands and management authorities; cumulative behaviour comes from
/// the group inheritance chain.
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
    let collection_entries = collection_entries(&groups, &default);
    let default_group_entries = default_group_entries(&groups);

    let output = quote! {
        //! Generated vanilla permission groups and collections. Do not edit by hand.

        use std::collections::BTreeMap;

        use steel_utils::Identifier;

        use crate::permission::{PermissionGroupConfig, PermissionGroupRule};

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
        /// Each collection holds the commands and management authorities declared for
        /// that level only. Levels inherit cumulatively through the generated groups.
        pub fn collections() -> Vec<(Identifier, Vec<PermissionGroupRule>)> {
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
        let constant = group_constant(name);
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

/// Emits the `(Identifier, Vec<PermissionGroupRule>)` entries for the code-owned collections.
///
/// Every level above the default group also carries the management authorities for
/// its own permissions, collection, group, and metadata. Higher levels inherit lower
/// ones, which scopes administration to a level and everything below it.
fn collection_entries(groups: &BTreeMap<String, Group>, default_group: &str) -> TokenStream {
    let mut tokens = TokenStream::new();
    for (name, group) in groups {
        let collection = collection_constant(name);
        let permissions = group
            .allow
            .iter()
            .map(|key| permission_rule(permission_identifier(key)));
        let management = (name != default_group)
            .then(|| management_identifiers(name, group, default_group))
            .into_iter()
            .flatten();
        let members = permissions.chain(management).collect::<Vec<_>>();
        tokens.extend(quote! {
            (
                #collection,
                vec![#(#members),*],
            ),
        });
    }
    tokens
}

/// Returns the management rules granted by one permission level.
fn management_identifiers(level: &str, group: &Group, default_group: &str) -> Vec<TokenStream> {
    let mut authorities = group
        .allow
        .iter()
        .map(|key| management_authority_identifier(key))
        .collect::<Vec<_>>();
    authorities.push(management_authority_identifier(&format!(
        "minecraft:{level}"
    )));
    authorities.push(group_authority_identifier(level));
    authorities.push(management_authority_identifier(&format!(
        "steel:permission.group.{level}"
    )));
    if level == DEFAULT_GROUP_MANAGER_LEVEL {
        authorities.push(group_authority_identifier(default_group));
        authorities.push(management_authority_identifier(&format!(
            "steel:permission.group.{default_group}"
        )));
    }
    let metadata = "permission.metadata";
    authorities.push(quote! {
        Identifier::new_static("steel", #metadata)
    });
    authorities.push(management_authority_identifier(&format!(
        "steel:{metadata}"
    )));

    let mut rules = authorities
        .into_iter()
        .map(permission_rule)
        .collect::<Vec<_>>();
    if level == OWNER_LEVEL {
        rules.extend([
            quote! { PermissionGroupRule::wildcard("steel", "permission.manage") },
            quote! { PermissionGroupRule::wildcard("steel", "permission.group") },
        ]);
    }
    rules
}

fn management_authority_identifier(name: &str) -> TokenStream {
    let (namespace, path) = name.split_once(':').unwrap_or(("minecraft", name));
    let value = format!("permission.manage.{namespace}.{path}");
    quote! {
        Identifier::new_static("steel", #value)
    }
}

fn group_authority_identifier(level: &str) -> TokenStream {
    let value = format!("permission.group.{level}");
    quote! {
        Identifier::new_static("steel", #value)
    }
}

/// Emits the default vanilla group configs referencing the generated collections.
///
/// Every level references its own collection, including levels with no vanilla
/// commands of their own: their collection still carries management authorities and
/// code-declared permissions such as `/perms`. Owners also receive the root management
/// wildcards in their collection.
fn default_group_entries(groups: &BTreeMap<String, Group>) -> TokenStream {
    let mut tokens = TokenStream::new();
    for (name, group) in groups {
        let inherits = group.inherits.clone().into_iter();
        let allow = iter::once(format!("#minecraft:{name}"));
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

fn group_constant(name: &str) -> Ident {
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

fn permission_identifier(name: &str) -> TokenStream {
    let (namespace, path) = name.split_once(':').unwrap_or(("minecraft", name));
    quote! {
        Identifier::new_static(#namespace, #path)
    }
}

fn permission_rule(identifier: TokenStream) -> TokenStream {
    quote! { PermissionGroupRule::permission(#identifier) }
}
