//! Code generation for item behaviors.
//!
//! Scans `src/behavior/items/**/*.rs` for structs annotated with `#[item_behavior]`,
//! cross-references with `classes.json`, and generates `register_item_behaviors()`.

use crate::common::{self, JsonArgField, JsonArgKind};
use proc_macro2::{Ident, Span};
use quote::quote;
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
use std::env;
use std::fs;

#[derive(Debug, Deserialize)]
pub struct ItemClass {
    pub name: String,
    pub class: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// --- Source scanning ---

/// A `when` condition parsed from `#[item_behavior(when(field = "value"))]`.
#[derive(Debug, Clone)]
struct WhenCondition {
    field: String,
    value: String,
    negated: bool,
}

impl WhenCondition {
    /// Evaluates the condition against a JSON extra map.
    fn matches(&self, extra: &serde_json::Map<String, serde_json::Value>) -> bool {
        let actual = extra
            .get(&self.field)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if self.negated {
            actual != self.value
        } else {
            actual == self.value
        }
    }
}

#[derive(Debug, Clone)]
struct DiscoveredItem {
    struct_name: String,
    /// All class names this struct handles (can be multiple).
    class_name: String,
    fields: Vec<JsonArgField>,
    when_conditions: Vec<WhenCondition>,
}

/// Parses `#[item_behavior(...)]` attribute.
///
/// Supports:
/// - `class = "ClassName"` (can appear multiple times)
/// - `when(field = "value")` or `when(field = "!value")` for conditional matching
fn extract_item_behavior_attr(
    attr: &syn::Attribute,
    struct_name: &str,
) -> (String, Vec<WhenCondition>) {
    let syn::Meta::List(meta) = &attr.meta else {
        return (String::new(), Vec::new());
    };

    let mut class_name = String::new();
    let mut when_conditions = Vec::new();

    meta.parse_nested_meta(|meta| {
        if meta.path.is_ident("class") {
            let value = meta.value()?;
            let lit: syn::LitStr = value.parse()?;
            class_name = lit.value();
        } else if meta.path.is_ident("when") {
            let content;
            syn::parenthesized!(content in meta.input);
            let field: Ident = content.parse()?;
            content.parse::<syn::Token![=]>()?;
            let lit: syn::LitStr = content.parse()?;
            let raw_value = lit.value();
            let (negated, value) = if let Some(stripped) = raw_value.strip_prefix('!') {
                (true, stripped.to_string())
            } else {
                (false, raw_value)
            };
            when_conditions.push(WhenCondition {
                field: field.to_string(),
                value,
                negated,
            });
        }
        Ok(())
    })
    .unwrap_or_else(|e| panic!("Failed to parse item_behavior attribute on `{struct_name}`: {e}"));
    if class_name.is_empty() {
        class_name = struct_name.to_string();
    }

    (class_name, when_conditions)
}

fn parse_item_behavior(s: &syn::ItemStruct) -> Option<DiscoveredItem> {
    let attr = s
        .attrs
        .iter()
        .find(|a| common::path_ends_with(a.path(), "item_behavior"))?;

    let struct_name = s.ident.to_string();
    let (class_name, when_conditions) = extract_item_behavior_attr(attr, &struct_name);

    assert!(
        !class_name.is_empty(),
        "item_behavior on `{struct_name}` must specify at least one `class = \"...\"`"
    );

    let mut fields = Vec::new();
    if let syn::Fields::Named(ref named) = s.fields {
        for field in &named.named {
            if let Some(json_arg) = common::parse_json_arg(field) {
                fields.push(json_arg);
            }
        }
    }

    Some(DiscoveredItem {
        struct_name,
        class_name,
        fields,
        when_conditions,
    })
}

/// Scans item behavior source files for `#[item_behavior]` annotations.
///
/// Returns a map from class name to a list of discovered items (multiple items
/// can map to the same class via `when` conditions, e.g., `BucketItem` → `FilledBucketBehavior` + `EmptyBucketBehavior`).
fn scan_item_behaviors() -> HashMap<String, Vec<DiscoveredItem>> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let pattern = format!("{manifest_dir}/src/behavior/items/**/*.rs");
    let mut discovered: HashMap<String, Vec<DiscoveredItem>> = HashMap::new();

    for entry in glob::glob(&pattern).expect("Failed to glob item behavior sources") {
        let path = entry.expect("Failed to read glob entry");
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
        let file = syn::parse_file(&content)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()));

        for item in &file.items {
            let syn::Item::Struct(s) = item else {
                continue;
            };
            let Some(info) = parse_item_behavior(s) else {
                continue;
            };

            discovered
                .entry(info.class_name.clone())
                .or_default()
                .push(info.clone());
        }
    }

    discovered
}

// --- Code generation ---

pub fn build(items: &[ItemClass]) -> String {
    let discovered = scan_item_behaviors();

    let mut type_imports = BTreeSet::new();
    let mut enum_imports = BTreeSet::new();
    let mut registrations = Vec::new();
    let mut matched_classes = BTreeSet::new();

    for item in items {
        let Some(candidates) = discovered.get(&item.class) else {
            continue;
        };

        // Find the matching candidate (evaluate `when` conditions)
        let Some(info) = candidates
            .iter()
            .find(|c| c.when_conditions.iter().all(|w| w.matches(&item.extra)))
        else {
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

        let registration = if info.fields.is_empty() {
            // Unit struct or struct with no json_args — instantiate directly
            quote! {
                registry.set_behavior(
                    &vanilla_items::ITEMS.#item_field,
                    Box::new(#struct_ident),
                );
            }
        } else {
            let args: Vec<_> = info
                .fields
                .iter()
                .map(|f| common::generate_arg(f, &item.extra, &item.name))
                .collect();

            quote! {
                registry.set_behavior(
                    &vanilla_items::ITEMS.#item_field,
                    Box::new(#struct_ident::new(#(#args),*)),
                );
            }
        };

        registrations.push(registration);
    }

    // Verify all discovered structs matched at least one JSON entry
    for (class_name, candidates) in &discovered {
        for info in candidates {
            assert!(
                matched_classes.contains(class_name),
                "Item behavior struct `{}` maps to class '{}' which doesn't exist in classes.json",
                info.struct_name,
                class_name
            );
        }
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
