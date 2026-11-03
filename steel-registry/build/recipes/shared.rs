//! Shared JSON validation and token generation for recipe families.

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde_json::{Map, Value};

pub(super) fn book_properties(category: TokenStream, value: &Value) -> TokenStream {
    let group = if let Some(group) = value
        .get("group")
        .and_then(Value::as_str)
        .filter(|group| !group.is_empty())
    {
        quote! { Some(#group.to_owned()) }
    } else {
        quote! { None }
    };
    let show_notification = value
        .get("show_notification")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    quote! {
        RecipeProperties {
            special: false,
            placement: None,
            recipe_book: Some(RecipeBookProperties {
                category: #category,
                group: #group,
                show_notification: #show_notification,
            }),
        }
    }
}

pub(super) fn result_tokens(value: &Value) -> TokenStream {
    let item = vanilla_ident(string_field(value, "id"));
    let count = value.get("count").and_then(Value::as_i64).unwrap_or(1) as i32;
    let Some(components) = value.get("components") else {
        return quote! { ItemStackTemplate::with_count(&vanilla_items::#item, #count) };
    };
    let components = components
        .as_object()
        .unwrap_or_else(|| panic!("Recipe result components are not an object: {components}"));
    assert_eq!(
        components.len(),
        1,
        "Recipe result has unsupported component set {components:?}"
    );
    let (key, component) = components
        .iter()
        .next()
        .unwrap_or_else(|| panic!("Recipe result components are empty: {components:?}"));
    let (component_type, component_value) = match key.as_str() {
        "minecraft:suspicious_stew_effects" => (
            quote! { SUSPICIOUS_STEW_EFFECTS },
            suspicious_stew_effects_tokens(component),
        ),
        "minecraft:potion_contents" => (
            quote! { POTION_CONTENTS },
            potion_contents_tokens(component),
        ),
        _ => panic!("Unsupported recipe result components {components:?}"),
    };
    quote! {{
        let value = #component_value;
        let components_hash = DataComponentPatch::compute_single_extracted_hash(
            vanilla_components::#component_type.key(),
            &value,
        );
        let mut components = DataComponentPatch::new();
        components.set(vanilla_components::#component_type, value);
        ItemStackTemplate::from_extracted(
            &vanilla_items::#item,
            #count,
            components,
            components_hash,
        )
    }}
}

fn suspicious_stew_effects_tokens(value: &Value) -> TokenStream {
    let effects = value
        .as_array()
        .unwrap_or_else(|| panic!("Suspicious stew effects are not an array: {value}"));
    let effects: Vec<_> = effects
        .iter()
        .map(|effect| {
            let effect_id = vanilla_ident(string_field(effect, "id"));
            let duration = effect
                .get("duration")
                .and_then(Value::as_i64)
                .unwrap_or(160) as i32;
            quote! { SuspiciousStewEffect::new(&vanilla_mob_effects::#effect_id, #duration) }
        })
        .collect();
    quote! { SuspiciousStewEffects::new(vec![#(#effects),*]) }
}

fn potion_contents_tokens(value: &Value) -> TokenStream {
    let contents = value
        .as_object()
        .unwrap_or_else(|| panic!("Potion contents are not an object: {value}"));
    assert!(
        contents.len() == 1 && contents.contains_key("potion"),
        "Unsupported extracted potion contents {contents:?}"
    );
    let potion = vanilla_ident(string_field(value, "potion"));
    quote! {
        PotionContents::new(
            Some(RegistryReference::new(&vanilla_potions::#potion)),
            None,
            Vec::new(),
            None,
        )
    }
}

/// Generates `Option<ItemStackTemplate>` tokens, mapping an empty extracted
/// result object to `None`.
pub(super) fn optional_result_tokens(value: &Value) -> TokenStream {
    if value.as_object().is_some_and(Map::is_empty) {
        return quote! { None };
    }
    let result = result_tokens(value);
    quote! { Some(#result) }
}

pub(super) fn optional_ingredient_tokens(value: Option<&Value>) -> TokenStream {
    if let Some(value) = value {
        let ingredient = ingredient_tokens(value);
        quote! { Some(#ingredient) }
    } else {
        quote! { None }
    }
}

pub(super) fn ingredient_tokens(value: &Value) -> TokenStream {
    match value {
        Value::String(value) => {
            if let Some(tag) = value.strip_prefix('#') {
                let identifier = identifier_tokens(tag);
                quote! { Ingredient::Tag(#identifier) }
            } else {
                let item = vanilla_ident(value);
                quote! { Ingredient::Item(&vanilla_items::#item) }
            }
        }
        Value::Array(values) => {
            let items: Vec<_> = values
                .iter()
                .map(|value| {
                    let item = value
                        .as_str()
                        .unwrap_or_else(|| panic!("Ingredient choice is not a string: {value}"));
                    let item = vanilla_ident(item);
                    quote! { &*vanilla_items::#item }
                })
                .collect();
            quote! {
                Ingredient::Choice(Box::leak(vec![#(#items),*].into_boxed_slice()))
            }
        }
        other => panic!("Unsupported extracted ingredient {other}"),
    }
}

fn identifier_tokens(identifier: &str) -> TokenStream {
    match identifier.split_once(':') {
        Some(("minecraft", path)) => quote! { Identifier::vanilla_static(#path) },
        Some((namespace, path)) => quote! { Identifier::new_static(#namespace, #path) },
        None => quote! { Identifier::vanilla_static(#identifier) },
    }
}

pub(super) fn vanilla_ident(identifier: &str) -> Ident {
    let path = identifier.strip_prefix("minecraft:").unwrap_or(identifier);
    assert!(
        !path.contains('/') && !identifier.contains(':') || identifier.starts_with("minecraft:"),
        "Expected a vanilla registry identifier, got {identifier}"
    );
    Ident::new(&path.to_shouty_snake_case(), Span::call_site())
}

pub(super) fn recipe_ident(name: &str) -> Ident {
    Ident::new(&name.to_shouty_snake_case(), Span::call_site())
}

pub(super) fn field<'a>(value: &'a Value, name: &str) -> &'a Value {
    value
        .get(name)
        .unwrap_or_else(|| panic!("Recipe is missing required field {name}: {value}"))
}

pub(super) fn string_field<'a>(value: &'a Value, name: &str) -> &'a str {
    field(value, name)
        .as_str()
        .unwrap_or_else(|| panic!("Recipe field {name} is not a string: {value}"))
}

pub(super) fn integer_field(value: &Value, name: &str) -> i32 {
    field(value, name)
        .as_i64()
        .unwrap_or_else(|| panic!("Recipe field {name} is not an integer: {value}")) as i32
}

pub(super) fn array_field<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    field(value, name)
        .as_array()
        .unwrap_or_else(|| panic!("Recipe field {name} is not an array: {value}"))
}

pub(super) fn object_field<'a>(value: &'a Value, name: &str) -> &'a Map<String, Value> {
    field(value, name)
        .as_object()
        .unwrap_or_else(|| panic!("Recipe field {name} is not an object: {value}"))
}
