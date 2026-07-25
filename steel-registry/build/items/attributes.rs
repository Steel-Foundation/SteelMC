use super::{
    Ident, Span, ToShoutySnakeCase, TokenStream, Value, entity_type_ref_token, identifier_token,
    quote, split_identifier,
};

pub(super) fn attribute_ref_token(s: &str) -> Option<TokenStream> {
    let (namespace, path) = split_identifier(s);
    if namespace != "minecraft" {
        return None;
    }

    let ident = Ident::new(&path.to_shouty_snake_case(), Span::call_site());
    Some(quote! { vanilla_attributes::#ident })
}

pub(super) fn attribute_modifier_operation_token(s: &str) -> Option<TokenStream> {
    match s {
        "add_value" => Some(quote! { vanilla_components::AttributeModifierOperation::AddValue }),
        "add_multiplied_base" => {
            Some(quote! { vanilla_components::AttributeModifierOperation::AddMultipliedBase })
        }
        "add_multiplied_total" => {
            Some(quote! { vanilla_components::AttributeModifierOperation::AddMultipliedTotal })
        }
        _ => None,
    }
}

pub(super) fn equipment_slot_group_token(s: &str) -> Option<TokenStream> {
    match s {
        "any" => Some(quote! { vanilla_components::EquipmentSlotGroup::Any }),
        "mainhand" | "main_hand" => {
            Some(quote! { vanilla_components::EquipmentSlotGroup::MainHand })
        }
        "offhand" | "off_hand" => Some(quote! { vanilla_components::EquipmentSlotGroup::OffHand }),
        "hand" => Some(quote! { vanilla_components::EquipmentSlotGroup::Hand }),
        "feet" => Some(quote! { vanilla_components::EquipmentSlotGroup::Feet }),
        "legs" => Some(quote! { vanilla_components::EquipmentSlotGroup::Legs }),
        "chest" => Some(quote! { vanilla_components::EquipmentSlotGroup::Chest }),
        "head" => Some(quote! { vanilla_components::EquipmentSlotGroup::Head }),
        "armor" => Some(quote! { vanilla_components::EquipmentSlotGroup::Armor }),
        "body" => Some(quote! { vanilla_components::EquipmentSlotGroup::Body }),
        "saddle" => Some(quote! { vanilla_components::EquipmentSlotGroup::Saddle }),
        _ => None,
    }
}

pub(super) fn generate_allowed_entities(value: &Value) -> TokenStream {
    match value.get("allowed_entities") {
        Some(Value::String(s)) if s.starts_with('#') => {
            let tag = identifier_token(s.trim_start_matches('#'));
            quote! { Some(vanilla_components::EquippableAllowedEntities::Tag(#tag)) }
        }
        Some(Value::String(s)) => {
            if let Some(entity_type) = entity_type_ref_token(s) {
                quote! {
                    Some(vanilla_components::EquippableAllowedEntities::Direct(vec![#entity_type]))
                }
            } else {
                quote! { None }
            }
        }
        Some(Value::Array(values)) => {
            let entity_types = values
                .iter()
                .filter_map(|value| value.as_str())
                .filter_map(entity_type_ref_token)
                .collect::<Vec<_>>();
            quote! {
                Some(vanilla_components::EquippableAllowedEntities::Direct(vec![#(#entity_types),*]))
            }
        }
        _ => quote! { None },
    }
}

pub(super) fn generate_attribute_modifiers_component(value: &Value) -> Option<TokenStream> {
    let entries = value.as_array()?;
    if entries.is_empty() {
        return None;
    }

    let modifiers = entries
        .iter()
        .map(generate_attribute_modifier_entry)
        .collect::<Vec<_>>();

    Some(quote! {
        vanilla_components::ItemAttributeModifiers {
            modifiers: vec![#(#modifiers),*],
        }
    })
}

pub(super) fn generate_attribute_modifier_entry(value: &Value) -> TokenStream {
    let attribute_value = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("attribute modifier entry missing type: {value:?}"));
    let attribute = attribute_ref_token(attribute_value)
        .unwrap_or_else(|| panic!("unknown item attribute modifier attribute: {attribute_value}"));
    let id_value = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("attribute modifier entry missing id: {value:?}"));
    let id = identifier_token(id_value);
    let amount = value
        .get("amount")
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("attribute modifier entry missing amount: {value:?}"));
    let operation_value = value
        .get("operation")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("attribute modifier entry missing operation: {value:?}"));
    let operation = attribute_modifier_operation_token(operation_value)
        .unwrap_or_else(|| panic!("unknown item attribute modifier operation: {operation_value}"));
    let slot_value = value.get("slot").and_then(Value::as_str).unwrap_or("any");
    let slot = equipment_slot_group_token(slot_value)
        .unwrap_or_else(|| panic!("unknown item attribute modifier slot group: {slot_value}"));
    let display = generate_attribute_modifier_display(value.get("display"));

    quote! {
        vanilla_components::ItemAttributeModifierEntry {
            attribute: #attribute,
            id: #id,
            amount: #amount,
            operation: #operation,
            slot: #slot,
            display: #display,
        }
    }
}

pub(super) fn generate_attribute_modifier_display(value: Option<&Value>) -> TokenStream {
    let Some(value) = value else {
        return quote! { vanilla_components::ItemAttributeModifierDisplay::Default };
    };
    let display_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("default");
    match display_type {
        "default" => quote! { vanilla_components::ItemAttributeModifierDisplay::Default },
        "hidden" => quote! { vanilla_components::ItemAttributeModifierDisplay::Hidden },
        _ => panic!("unknown item attribute modifier display type: {display_type}"),
    }
}
