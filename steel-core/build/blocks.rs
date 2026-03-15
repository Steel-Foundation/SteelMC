//! Code generation for block behaviors.
//!
//! Scans `src/behavior/blocks/**/*.rs` for structs annotated with `#[block_behavior]`,
//! cross-references with `classes.json`, and generates `register_block_behaviors()`.

use heck::{ToPascalCase, ToShoutySnakeCase};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
use std::env;
use std::fs;

#[derive(Debug, Deserialize)]
pub struct BlockClass {
    pub name: String,
    pub class: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn to_const_ident(name: &str) -> Ident {
    Ident::new(&name.to_shouty_snake_case(), Span::call_site())
}

// --- Source scanning ---

#[derive(Debug, Clone)]
enum JsonArgKind {
    /// Raw JSON value → token literal (handles numbers, strings, bools)
    Value,
    /// JSON string → `module::IDENT`. Stores the module name.
    Registry(String),
    /// JSON string → `EnumType::Variant` (`PascalCase`). Stores the enum type name.
    Enum(String),
}

#[derive(Debug, Clone)]
struct JsonArgField {
    field_name: String,
    kind: JsonArgKind,
    json_name: Option<String>,
    is_ref: bool,
}

#[derive(Debug)]
struct DiscoveredBlock {
    struct_name: String,
    class_name: String,
    fields: Vec<JsonArgField>,
}

fn extract_class_name(attr: &syn::Attribute) -> Option<String> {
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
    .unwrap_or_else(|e| panic!("Failed to parse block_behavior attribute: {e}"));
    class_name
}

fn parse_json_arg(field: &syn::Field) -> Option<JsonArgField> {
    let attr = field.attrs.iter().find(|a| a.path().is_ident("json_arg"))?;
    let field_name = field.ident.as_ref()?.to_string();

    let mut kind = None;
    let mut json_name = None;
    let mut is_ref = false;

    if let syn::Meta::List(meta) = &attr.meta {
        meta.parse_nested_meta(|meta| {
            if meta.path.is_ident("value") {
                kind = Some(JsonArgKind::Value);
            } else if meta.path.is_ident("r#enum") || meta.path.is_ident("enum") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                kind = Some(JsonArgKind::Enum(lit.value()));
            } else if meta.path.is_ident("r#ref") || meta.path.is_ident("ref") {
                is_ref = true;
            } else if meta.path.is_ident("json") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                json_name = Some(lit.value());
            } else if let Some(ident) = meta.path.get_ident() {
                kind = Some(JsonArgKind::Registry(ident.to_string()));
            }
            Ok(())
        })
        .unwrap_or_else(|e| panic!("Failed to parse json_arg: {e}"));
    }

    let kind = kind.unwrap_or_else(|| {
        panic!("json_arg on field '{field_name}' must specify a kind (value, enum, or a registry module name)")
    });

    Some(JsonArgField {
        field_name,
        kind,
        json_name,
        is_ref,
    })
}

fn parse_block_behavior(s: &syn::ItemStruct) -> Option<DiscoveredBlock> {
    let attr = s
        .attrs
        .iter()
        .find(|a| a.path().is_ident("block_behavior"))?;
    let class_name = extract_class_name(attr).unwrap_or_else(|| s.ident.to_string());

    let mut fields = Vec::new();
    if let syn::Fields::Named(ref named) = s.fields {
        for field in &named.named {
            if let Some(json_arg) = parse_json_arg(field) {
                fields.push(json_arg);
            }
        }
    }

    Some(DiscoveredBlock {
        struct_name: s.ident.to_string(),
        class_name,
        fields,
    })
}

fn scan_block_behaviors() -> HashMap<String, DiscoveredBlock> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let pattern = format!("{manifest_dir}/src/behavior/blocks/**/*.rs");
    let mut discovered = HashMap::new();

    for entry in glob::glob(&pattern).expect("Failed to glob block behavior sources") {
        let path = entry.expect("Failed to read glob entry");
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
        let file = syn::parse_file(&content)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()));

        for item in &file.items {
            if let syn::Item::Struct(s) = item
                && let Some(block) = parse_block_behavior(s)
            {
                discovered.insert(block.class_name.clone(), block);
            }
        }
    }

    discovered
}

// --- Code generation ---

fn get_json_str<'a>(block: &'a BlockClass, key: &str) -> &'a str {
    block
        .extra
        .get(key)
        .unwrap_or_else(|| panic!("Block '{}' missing JSON field '{key}'", block.name))
        .as_str()
        .unwrap_or_else(|| {
            panic!(
                "JSON field '{key}' for block '{}' must be a string",
                block.name
            )
        })
}

fn get_json_value<'a>(block: &'a BlockClass, key: &str) -> &'a serde_json::Value {
    block
        .extra
        .get(key)
        .unwrap_or_else(|| panic!("Block '{}' missing JSON field '{key}'", block.name))
}

fn json_value_to_tokens(value: &serde_json::Value, block_name: &str, key: &str) -> TokenStream {
    match value {
        serde_json::Value::Number(n) => {
            let n = n.as_i64().unwrap_or_else(|| {
                panic!("JSON field '{key}' for block '{block_name}' must be an integer")
            });
            let lit = proc_macro2::Literal::i32_suffixed(n as i32);
            quote! { #lit }
        }
        serde_json::Value::String(s) => quote! { #s },
        serde_json::Value::Bool(b) => quote! { #b },
        _ => panic!("Unsupported JSON value type for block '{block_name}' field '{key}'"),
    }
}

fn generate_arg(field: &JsonArgField, block: &BlockClass) -> TokenStream {
    let json_key = field.json_name.as_deref().unwrap_or(&field.field_name);

    let tokens = match &field.kind {
        JsonArgKind::Value => {
            let value = get_json_value(block, json_key);
            json_value_to_tokens(value, &block.name, json_key)
        }
        JsonArgKind::Registry(module) => {
            let module_ident = Ident::new(module, Span::call_site());
            let ident = to_const_ident(get_json_str(block, json_key));
            quote! { #module_ident::#ident }
        }
        JsonArgKind::Enum(enum_type) => {
            let enum_ident = Ident::new(enum_type, Span::call_site());
            let variant_str = get_json_str(block, json_key);
            let variant = Ident::new(&variant_str.to_pascal_case(), Span::call_site());
            quote! { #enum_ident::#variant }
        }
    };

    if field.is_ref {
        quote! { &#tokens }
    } else {
        tokens
    }
}

pub fn build(blocks: &[BlockClass]) -> String {
    let discovered = scan_block_behaviors();

    let mut block_type_imports = BTreeSet::new();
    let mut registrations = Vec::new();
    let mut matched_classes = BTreeSet::new();

    for block in blocks {
        let Some(info) = discovered.get(&block.class) else {
            continue;
        };
        matched_classes.insert(&info.class_name);

        let struct_ident = Ident::new(&info.struct_name, Span::call_site());
        let const_ident = to_const_ident(&block.name);

        block_type_imports.insert(info.struct_name.clone());

        for field in &info.fields {
            if let JsonArgKind::Enum(ref enum_type) = field.kind {
                block_type_imports.insert(enum_type.clone());
            }
        }

        let mut args = Vec::new();
        for field in &info.fields {
            args.push(generate_arg(field, block));
        }

        let registration = quote! {
            registry.set_behavior(
                vanilla_blocks::#const_ident,
                Box::new(#struct_ident::new(vanilla_blocks::#const_ident #(, #args)*)),
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

    // Build imports
    let block_imports: Vec<_> = block_type_imports
        .iter()
        .map(|name| Ident::new(name, Span::call_site()))
        .collect();

    let output = quote! {
        //! Generated block behavior assignments.

        use steel_registry::{sound_events, vanilla_fluids, vanilla_blocks};
        use crate::behavior::BlockBehaviorRegistry;
        use crate::behavior::blocks::{#(#block_imports),*};

        pub fn register_block_behaviors(registry: &mut BlockBehaviorRegistry) {
            #(#registrations)*
        }
    };

    output.to_string()
}
