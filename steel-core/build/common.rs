//! Shared types and helpers for block/item behavior code generation.
//!
//! Used by both `blocks.rs` and `items.rs` build scripts to parse `#[json_arg]`
//! attributes and generate constructor arguments from `classes.json`.

use heck::{ToPascalCase, ToShoutySnakeCase};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use std::collections::HashMap;
use std::{env, fs};

/// Checks if an attribute path ends with the given identifier.
///
/// Handles both `#[item_behavior]` and `#[steel_macros::item_behavior]`.
pub(crate) fn path_ends_with(path: &syn::Path, name: &str) -> bool {
    path.segments.last().is_some_and(|s| s.ident == name)
}

/// Converts a name to a `SCREAMING_SNAKE_CASE` identifier.
pub(crate) fn to_const_ident(name: &str) -> Ident {
    Ident::new(&name.to_shouty_snake_case(), Span::call_site())
}

// --- JSON arg parsing ---

/// How a `#[json_arg]` field maps JSON data to Rust tokens.
#[derive(Debug, Clone)]
pub(crate) enum JsonArgKind {
    /// Raw JSON value → token literal (handles numbers, strings, bools)
    Value,
    /// JSON string → `module::IDENT`. Stores the module name.
    Registry(String),
    /// JSON string → `EnumType::Variant` (`PascalCase`).
    /// `module_path` is the full import path (e.g. `"steel_registry::blocks::properties"`).
    Enum {
        type_name: String,
        module_path: Option<String>,
    },
}

/// A parsed `#[json_arg(...)]` field.
#[derive(Debug, Clone)]
pub(crate) struct JsonArgField {
    pub field_name: String,
    pub kind: JsonArgKind,
    pub json_name: Option<String>,
    pub is_ref: bool,
    pub optional_sentinel: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct DiscoveredObject {
    pub(crate) struct_name: String,
    pub(crate) class_name: String,
    pub(crate) fields: Vec<JsonArgField>,
}

pub(crate) fn parse_object_behavior(
    s: &syn::ItemStruct,
    attribute_name: &str,
) -> Option<DiscoveredObject> {
    let attr = s
        .attrs
        .iter()
        .find(|a| path_ends_with(a.path(), attribute_name))?;
    let class_name = extract_class_name(attr, attribute_name).unwrap_or(s.ident.to_string());

    let mut fields = Vec::new();
    if let syn::Fields::Named(ref named) = s.fields {
        for field in &named.named {
            if let Some(json_arg) = parse_json_arg(field) {
                fields.push(json_arg);
            }
        }
    }

    Some(DiscoveredObject {
        struct_name: s.ident.to_string(),
        class_name,
        fields,
    })
}

const KNOWN_REGISTRIES: &[&str] = &[
    "vanilla_blocks",
    "vanilla_items",
    "vanilla_fluids",
    "sound_events",
];

/// Parses a `#[json_arg(...)]` attribute from a `syn::Field`.
pub(crate) fn parse_json_arg(field: &syn::Field) -> Option<JsonArgField> {
    let attr = field.attrs.iter().find(|a| a.path().is_ident("json_arg"))?;
    let field_name = field.ident.as_ref()?.to_string();

    let mut kind = None;
    let mut json_name = None;
    let mut is_ref = false;
    let mut optional_sentinel = None;
    let mut module_path = None;

    if let syn::Meta::List(meta) = &attr.meta {
        meta.parse_nested_meta(|meta| {
            if meta.path.is_ident("value") {
                kind = Some(JsonArgKind::Value);
            } else if meta.path.is_ident("r#enum") || meta.path.is_ident("enum") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                kind = Some(JsonArgKind::Enum {
                    type_name: lit.value(),
                    module_path: None,
                });
            } else if meta.path.is_ident("module") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                module_path = Some(lit.value());
            } else if meta.path.is_ident("r#ref") || meta.path.is_ident("ref") {
                is_ref = true;
            } else if meta.path.is_ident("json") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                json_name = Some(lit.value());
            } else if meta.path.is_ident("optional") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                optional_sentinel = Some(lit.value());
            } else if let Some(ident) = meta.path.get_ident() {
                let name = ident.to_string();
                assert!(
                    KNOWN_REGISTRIES.contains(&name.as_str()),
                    "Unknown json_arg attribute '{name}' on field '{field_name}'. \
                     Expected: value, enum, ref, json, optional, or a registry module ({}).",
                    KNOWN_REGISTRIES.join(", ")
                );
                kind = Some(JsonArgKind::Registry(name));
            }
            Ok(())
        })
        .unwrap_or_else(|e| panic!("Failed to parse json_arg: {e}"));
    }

    // Attach module_path to enum kind if both were specified
    let kind = match kind {
        Some(JsonArgKind::Enum { type_name, .. }) => Some(JsonArgKind::Enum {
            type_name,
            module_path,
        }),
        other => other,
    };

    let kind = kind.unwrap_or_else(|| {
        panic!("json_arg on field '{field_name}' must specify a kind (value, enum, or a registry module name)")
    });

    Some(JsonArgField {
        field_name,
        kind,
        json_name,
        is_ref,
        optional_sentinel,
    })
}

// --- JSON helpers ---

/// Gets a string value from a JSON extra map.
pub(crate) fn get_json_str<'a>(
    extra: &'a serde_json::Map<String, serde_json::Value>,
    entry_name: &str,
    key: &str,
) -> &'a str {
    extra
        .get(key)
        .unwrap_or_else(|| panic!("Entry '{entry_name}' missing JSON field '{key}'"))
        .as_str()
        .unwrap_or_else(|| panic!("JSON field '{key}' for entry '{entry_name}' must be a string"))
}

/// Gets a raw JSON value from a JSON extra map.
pub(crate) fn get_json_value<'a>(
    extra: &'a serde_json::Map<String, serde_json::Value>,
    entry_name: &str,
    key: &str,
) -> &'a serde_json::Value {
    extra
        .get(key)
        .unwrap_or_else(|| panic!("Entry '{entry_name}' missing JSON field '{key}'"))
}

/// Converts a JSON value to a token literal.
pub(crate) fn json_value_to_tokens(
    value: &serde_json::Value,
    entry_name: &str,
    key: &str,
) -> TokenStream {
    match value {
        serde_json::Value::Number(n) => {
            let n = n.as_i64().unwrap_or_else(|| {
                panic!("JSON field '{key}' for entry '{entry_name}' must be an integer")
            });
            let n = i32::try_from(n).unwrap_or_else(|_| {
                panic!("JSON field '{key}' for entry '{entry_name}' overflows i32: {n}")
            });
            let lit = proc_macro2::Literal::i32_suffixed(n);
            quote! { #lit }
        }
        serde_json::Value::String(s) => quote! { #s },
        serde_json::Value::Bool(b) => quote! { #b },
        _ => panic!("Unsupported JSON value type for entry '{entry_name}' field '{key}'"),
    }
}

// --- Code generation ---

/// Generates a constructor argument token stream from a `JsonArgField` and JSON data.
///
/// Registry access modes:
/// - `vanilla_items` → `vanilla_items::ITEMS.lowercase_field` (struct field access)
/// - Other registries → `module::SCREAMING_SNAKE` (module constant access)
pub(crate) fn generate_arg(
    field: &JsonArgField,
    extra: &serde_json::Map<String, serde_json::Value>,
    entry_name: &str,
) -> TokenStream {
    let json_key = field.json_name.as_deref().unwrap_or(&field.field_name);

    // For optional fields, check the sentinel before computing the registry token.
    if let Some(sentinel) = &field.optional_sentinel {
        let raw = get_json_str(extra, entry_name, json_key);
        if raw == sentinel {
            return quote! { None };
        }
    }

    let tokens = match &field.kind {
        JsonArgKind::Value => {
            let value = get_json_value(extra, entry_name, json_key);
            json_value_to_tokens(value, entry_name, json_key)
        }
        JsonArgKind::Registry(module) => {
            let name = get_json_str(extra, entry_name, json_key);
            // vanilla_items uses a LazyLock<Items> struct with field access (ITEMS.stone),
            // while other registries use module-level constants (vanilla_blocks::STONE).
            if module == "vanilla_items" {
                let field_ident = Ident::new(name, Span::call_site());
                quote! { vanilla_items::ITEMS.#field_ident }
            } else {
                let module_ident = Ident::new(module, Span::call_site());
                let const_ident = to_const_ident(name);
                quote! { #module_ident::#const_ident }
            }
        }
        JsonArgKind::Enum { type_name, .. } => {
            let enum_ident = Ident::new(type_name, Span::call_site());
            let variant_str = get_json_str(extra, entry_name, json_key);
            let variant = Ident::new(&variant_str.to_pascal_case(), Span::call_site());
            quote! { #enum_ident::#variant }
        }
    };

    let result = if field.is_ref {
        quote! { &#tokens }
    } else {
        tokens
    };

    if field.optional_sentinel.is_some() {
        quote! { Some(#result) }
    } else {
        result
    }
}

pub(crate) fn extract_class_name(attr: &syn::Attribute, attribute_name: &str) -> Option<String> {
    let syn::Meta::List(meta) = &attr.meta else {
        return None;
    };

    let mut class_name = None;
    meta.parse_nested_meta(|meta| {
        if meta.path.is_ident("class") {
            let value = meta.value()?;
            let lit: syn::LitStr = value.parse()?;
            class_name = Some(lit.value());
        }
        Ok(())
    })
    .unwrap_or_else(|e| panic!("Failed to parse {attribute_name} attribute: {e}"));
    class_name
}

/// Scans behavior source files for annotated structs (e.g. `#[block_behavior]`, `#[item_behavior]`).
pub(crate) fn scan_object_behaviors(
    folder: &str,
    attribute_name: &str,
) -> HashMap<String, DiscoveredObject> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let pattern = format!("{manifest_dir}/src/behavior/{folder}/**/*.rs");
    let mut discovered: HashMap<String, DiscoveredObject> = HashMap::new();

    for entry in
        glob::glob(&pattern).unwrap_or_else(|_| panic!("Failed to glob {folder} behavior sources"))
    {
        let path = entry.expect("Failed to read glob entry");
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
        let file = syn::parse_file(&content)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()));

        for item in &file.items {
            if let syn::Item::Struct(s) = item
                && let Some(info) = parse_object_behavior(s, attribute_name)
            {
                let class_name = info.class_name.clone();
                discovered.insert(class_name, info);
            }
        }
    }

    discovered
}
