#![expect(
    clippy::unwrap_used,
    reason = "build script must fail immediately on invalid extracted item data"
)]

use std::{collections::BTreeMap, fs, str::FromStr};

use crate::generator_functions::generate_sound_event_ref;
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use serde_json::Value;
use steel_utils::Identifier;

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
#[expect(
    dead_code,
    reason = "extracted item JSON includes fields not used by current item generation"
)]
pub struct Item {
    pub id: u16,
    pub name: String,
    #[serde(default)]
    pub components: BTreeMap<String, Value>,
    #[serde(default)]
    pub block_item: Option<String>,
    #[serde(default)]
    pub is_double: bool,
    #[serde(default)]
    pub is_scaffolding: bool,
    #[serde(default)]
    pub is_water_placable: bool,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Items {
    pub items: Vec<Item>,
}

fn get_component_ident(name: &str) -> Option<Ident> {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let shouty_name = name.to_shouty_snake_case();
    Some(Ident::new(&shouty_name, Span::call_site()))
}

/// Generates the `TokenStream` for a Tool component from JSON data.
fn generate_tool_component(value: &Value) -> TokenStream {
    let rules = value
        .get("rules")
        .and_then(|r| r.as_array())
        .unwrap_or_else(|| panic!("tool component must contain a rules array"))
        .iter()
        .map(generate_tool_rule)
        .collect::<Vec<_>>();

    let default_mining_speed = value.get("default_mining_speed").map_or(1.0, |value| {
        value
            .as_f64()
            .unwrap_or_else(|| panic!("tool default_mining_speed must be a number"))
    }) as f32;

    let damage_per_block = value.get("damage_per_block").map_or(1, |value| {
        let value = value
            .as_i64()
            .unwrap_or_else(|| panic!("tool damage_per_block must be an integer"));
        i32::try_from(value)
            .unwrap_or_else(|_| panic!("tool damage_per_block is outside the i32 range: {value}"))
    });
    assert!(
        damage_per_block >= 0,
        "tool damage_per_block must be non-negative"
    );

    let can_destroy_blocks_in_creative =
        value
            .get("can_destroy_blocks_in_creative")
            .is_none_or(|value| {
                value.as_bool().unwrap_or_else(|| {
                    panic!("tool can_destroy_blocks_in_creative must be a boolean")
                })
            });

    quote! {
        vanilla_components::Tool {
            rules: vec![#(#rules),*],
            default_mining_speed: #default_mining_speed,
            damage_per_block: #damage_per_block,
            can_destroy_blocks_in_creative: #can_destroy_blocks_in_creative,
        }
    }
}

fn block_ref_token(value: &str) -> TokenStream {
    let id = Identifier::from_str(value)
        .unwrap_or_else(|error| panic!("invalid tool block id {value:?}: {error}"));
    assert_eq!(
        id.namespace.as_ref(),
        "minecraft",
        "vanilla tool rules must reference minecraft blocks: {id}"
    );
    let ident = Ident::new(&id.path.to_shouty_snake_case(), Span::call_site());
    quote! { &vanilla_blocks::#ident }
}

fn split_identifier(s: &str) -> (&str, &str) {
    s.split_once(':').unwrap_or(("minecraft", s))
}

fn identifier_token(s: &str) -> TokenStream {
    let (namespace, path) = split_identifier(s);
    quote! { Identifier::new_static(#namespace, #path) }
}

fn entity_type_ref_token(s: &str) -> Option<TokenStream> {
    let (namespace, path) = split_identifier(s);
    if namespace != "minecraft" {
        return None;
    }

    let ident = Ident::new(&path.to_shouty_snake_case(), Span::call_site());
    Some(quote! { &vanilla_entities::#ident })
}

fn registry_sound_event_holder_token(sound: &str, field: &str) -> TokenStream {
    let id = Identifier::from_str(sound).unwrap_or_else(|error| {
        panic!("invalid sound event id {sound:?} in equippable field {field}: {error}")
    });
    let sound = generate_sound_event_ref(&id);
    quote! { crate::sound_event::SoundEventHolder::registry(#sound) }
}

fn sound_event_holder_token(value: &Value, field: &str, default: &str) -> TokenStream {
    let Some(value) = value.get(field) else {
        return registry_sound_event_holder_token(default, field);
    };

    if let Some(sound) = value.as_str() {
        return registry_sound_event_holder_token(sound, field);
    }

    let Some(sound) = value.as_object() else {
        panic!("equippable field {field} must be a sound id string or direct sound object");
    };
    let sound_id_value = sound
        .get("sound_id")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("direct equippable sound field {field} missing sound_id"));
    Identifier::from_str(sound_id_value).unwrap_or_else(|error| {
        panic!("invalid direct equippable sound id {sound_id_value:?} in field {field}: {error}")
    });
    let sound_id = identifier_token(sound_id_value);
    let fixed_range = sound.get("range").map_or_else(
        || quote! { None },
        |range| {
            let range = range.as_f64().unwrap_or_else(|| {
                panic!("direct equippable sound field {field} range must be a number")
            }) as f32;
            quote! { Some(#range) }
        },
    );

    quote! {
        crate::sound_event::SoundEventHolder::Direct {
            sound_id: #sound_id,
            fixed_range: #fixed_range,
        }
    }
}

fn damage_type_ref_token(value: &str) -> TokenStream {
    let id = Identifier::from_str(value)
        .unwrap_or_else(|error| panic!("invalid damage_type component id {value:?}: {error}"));
    assert_eq!(
        id.namespace.as_ref(),
        "minecraft",
        "vanilla item damage_type references must use the minecraft namespace: {id}"
    );

    let ident = Ident::new(&id.path.to_shouty_snake_case(), Span::call_site());
    quote! { &crate::vanilla_damage_types::#ident }
}

fn optional_identifier_token(value: &Value, field: &str) -> TokenStream {
    value
        .get(field)
        .and_then(|value| value.as_str())
        .map_or_else(
            || quote! { None },
            |id| {
                let id = identifier_token(id);
                quote! { Some(#id) }
            },
        )
}

fn attribute_ref_token(s: &str) -> Option<TokenStream> {
    let (namespace, path) = split_identifier(s);
    if namespace != "minecraft" {
        return None;
    }

    let ident = Ident::new(&path.to_shouty_snake_case(), Span::call_site());
    Some(quote! { vanilla_attributes::#ident })
}

fn attribute_modifier_operation_token(s: &str) -> Option<TokenStream> {
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

fn equipment_slot_group_token(s: &str) -> Option<TokenStream> {
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

fn generate_allowed_entities(value: &Value) -> TokenStream {
    match value.get("allowed_entities") {
        Some(Value::String(s)) if s.starts_with('#') => {
            let tag = identifier_token(s.trim_start_matches('#'));
            quote! { Some(vanilla_components::EquippableAllowedEntities::Tag(#tag)) }
        }
        Some(Value::String(s)) => {
            if let Some(entity_type) = entity_type_ref_token(s) {
                quote! {
                    Some(vanilla_components::EquippableAllowedEntities::EntityTypes(vec![#entity_type]))
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
                Some(vanilla_components::EquippableAllowedEntities::EntityTypes(vec![#(#entity_types),*]))
            }
        }
        _ => quote! { None },
    }
}

fn generate_attribute_modifiers_component(value: &Value) -> Option<TokenStream> {
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

fn generate_attribute_modifier_entry(value: &Value) -> TokenStream {
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

fn generate_attribute_modifier_display(value: Option<&Value>) -> TokenStream {
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

fn generate_weapon_component(value: &Value) -> TokenStream {
    let item_damage_per_attack = value
        .get("item_damage_per_attack")
        .and_then(Value::as_i64)
        .unwrap_or(1) as i32;
    let disable_blocking_for_seconds = value
        .get("disable_blocking_for_seconds")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;

    quote! {
        vanilla_components::Weapon {
            item_damage_per_attack: #item_damage_per_attack,
            disable_blocking_for_seconds: #disable_blocking_for_seconds,
        }
    }
}

fn generate_attack_range_component(value: &Value) -> TokenStream {
    let min_reach = value
        .get("min_reach")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    let max_reach = value
        .get("max_reach")
        .and_then(Value::as_f64)
        .unwrap_or(3.0) as f32;
    let min_creative_reach = value
        .get("min_creative_reach")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    let max_creative_reach = value
        .get("max_creative_reach")
        .and_then(Value::as_f64)
        .unwrap_or(5.0) as f32;
    let hitbox_margin = value
        .get("hitbox_margin")
        .and_then(Value::as_f64)
        .unwrap_or(0.3) as f32;
    let mob_factor = value
        .get("mob_factor")
        .and_then(Value::as_f64)
        .unwrap_or(1.0) as f32;

    quote! {
        vanilla_components::AttackRange {
            min_reach: #min_reach,
            max_reach: #max_reach,
            min_creative_reach: #min_creative_reach,
            max_creative_reach: #max_creative_reach,
            hitbox_margin: #hitbox_margin,
            mob_factor: #mob_factor,
        }
    }
}

fn optional_sound_event_holder_token(value: &Value, field: &str) -> TokenStream {
    let Some(value) = value.get(field) else {
        return quote! { None };
    };

    if let Some(sound) = value.as_str() {
        let id = Identifier::from_str(sound).unwrap_or_else(|error| {
            panic!("invalid sound event id {sound:?} in piercing weapon field {field}: {error}")
        });
        let sound = generate_sound_event_ref(&id);
        return quote! { Some(crate::sound_event::SoundEventHolder::registry(#sound)) };
    }

    let Some(sound) = value.as_object() else {
        panic!("piercing weapon field {field} must be a sound id string or direct sound object");
    };
    let sound_id_value = sound
        .get("sound_id")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("direct piercing weapon sound field {field} missing sound_id"));
    Identifier::from_str(sound_id_value).unwrap_or_else(|error| {
        panic!(
            "invalid direct piercing weapon sound id {sound_id_value:?} in field {field}: {error}"
        )
    });
    let sound_id = identifier_token(sound_id_value);
    let fixed_range = sound.get("range").map_or_else(
        || quote! { None },
        |range| {
            let range = range.as_f64().unwrap_or_else(|| {
                panic!("direct piercing weapon sound field {field} range must be a number")
            }) as f32;
            quote! { Some(#range) }
        },
    );
    quote! {
        Some(crate::sound_event::SoundEventHolder::Direct {
            sound_id: #sound_id,
            fixed_range: #fixed_range,
        })
    }
}

fn generate_piercing_weapon_component(value: &Value) -> TokenStream {
    let deals_knockback = value
        .get("deals_knockback")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let dismounts = value
        .get("dismounts")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let sound = optional_sound_event_holder_token(value, "sound");
    let hit_sound = optional_sound_event_holder_token(value, "hit_sound");

    quote! {
        vanilla_components::PiercingWeapon {
            deals_knockback: #deals_knockback,
            dismounts: #dismounts,
            sound: #sound,
            hit_sound: #hit_sound,
        }
    }
}

/// Generates the `TokenStream` for a single `ToolRule` from JSON data.
fn generate_tool_rule(rule: &Value) -> TokenStream {
    let blocks_token = match rule.get("blocks") {
        Some(Value::String(value)) if value.starts_with('#') => {
            let tag_value = value.trim_start_matches('#');
            Identifier::from_str(tag_value)
                .unwrap_or_else(|error| panic!("invalid tool block tag {value:?}: {error}"));
            let tag = identifier_token(tag_value);
            quote! { vanilla_components::ToolRuleBlocks::Tag(#tag) }
        }
        Some(Value::String(value)) => {
            let block = block_ref_token(value);
            quote! { vanilla_components::ToolRuleBlocks::Blocks(vec![#block]) }
        }
        Some(Value::Array(values)) => {
            let blocks = values
                .iter()
                .map(|value| {
                    let value = value
                        .as_str()
                        .unwrap_or_else(|| panic!("tool rule block list entries must be strings"));
                    assert!(
                        !value.starts_with('#'),
                        "tool rule direct block lists cannot contain tags: {value}"
                    );
                    block_ref_token(value)
                })
                .collect::<Vec<_>>();
            quote! { vanilla_components::ToolRuleBlocks::Blocks(vec![#(#blocks),*]) }
        }
        _ => panic!("tool rule must contain blocks as a string or string array"),
    };

    let speed_token = if let Some(value) = rule.get("speed") {
        let speed = value
            .as_f64()
            .unwrap_or_else(|| panic!("tool rule speed must be a number"))
            as f32;
        assert!(
            speed.is_finite() && speed > 0.0,
            "tool rule speed must be a positive finite float"
        );
        quote! { Some(#speed) }
    } else {
        quote! { None }
    };

    let correct_for_drops_token = if let Some(value) = rule.get("correct_for_drops") {
        let correct = value
            .as_bool()
            .unwrap_or_else(|| panic!("tool rule correct_for_drops must be a boolean"));
        quote! { Some(#correct) }
    } else {
        quote! { None }
    };

    quote! {
        vanilla_components::ToolRule {
            blocks: #blocks_token,
            speed: #speed_token,
            correct_for_drops: #correct_for_drops_token,
        }
    }
}

/// Returns the crafting remainder item key for a given item, if any.
/// Based on vanilla Minecraft's `Item.Properties.craftRemainder()` calls.
fn get_craft_remainder(item_name: &str) -> Option<&'static str> {
    match item_name {
        // Buckets return empty bucket
        "water_bucket"
        | "lava_bucket"
        | "milk_bucket"
        | "powder_snow_bucket"
        | "pufferfish_bucket"
        | "salmon_bucket"
        | "cod_bucket"
        | "tropical_fish_bucket"
        | "axolotl_bucket"
        | "tadpole_bucket" => Some("bucket"),
        // Bottles return empty glass bottle
        "dragon_breath" | "honey_bottle" => Some("glass_bottle"),
        // Potions also return glass bottles when used in crafting
        "potion" => Some("glass_bottle"),
        _ => None,
    }
}

fn generate_builder_calls(item: &Item) -> Vec<TokenStream> {
    let mut builder_calls = Vec::new();

    for (key, value) in &item.components {
        let component_ident = if let Some(ident) = get_component_ident(key) {
            ident
        } else {
            continue;
        };

        match key.as_str() {
            "minecraft:max_stack_size" => {
                let val = value.as_i64().unwrap() as i32;
                if val != 64 {
                    builder_calls.push(
                        quote! { .builder_set(vanilla_components::#component_ident, Some(#val)) },
                    );
                }
            }
            "minecraft:max_damage" => {
                let val = value.as_i64().unwrap() as i32;
                builder_calls.push(
                    quote! { .builder_set(vanilla_components::#component_ident, Some(#val)) },
                );
            }
            "minecraft:damage" => {
                let val = value.as_i64().unwrap() as i32;
                builder_calls.push(
                    quote! { .builder_set(vanilla_components::#component_ident, Some(#val)) },
                );
            }
            "minecraft:repair_cost" => {
                let val = value.as_i64().unwrap() as i32;
                if val != 0 {
                    builder_calls.push(
                        quote! { .builder_set(vanilla_components::#component_ident, Some(#val)) },
                    );
                }
            }
            "minecraft:unbreakable" => {
                builder_calls
                    .push(quote! { .builder_set(vanilla_components::#component_ident, Some(())) });
            }
            "minecraft:glider" => {
                builder_calls
                    .push(quote! { .builder_set(vanilla_components::#component_ident, Some(())) });
            }
            "minecraft:enchantment_glint_override" => {
                let val = value.as_bool().unwrap();
                builder_calls.push(
                    quote! { .builder_set(vanilla_components::#component_ident, Some(#val)) },
                );
            }
            "minecraft:equippable" => {
                // Parse the equippable component to get the slot
                if let Some(slot_str) = value.get("slot").and_then(|s| s.as_str()) {
                    let slot_variant = match slot_str {
                        "head" => quote! { vanilla_components::EquipmentSlot::Head },
                        "chest" => quote! { vanilla_components::EquipmentSlot::Chest },
                        "legs" => quote! { vanilla_components::EquipmentSlot::Legs },
                        "feet" => quote! { vanilla_components::EquipmentSlot::Feet },
                        "body" => quote! { vanilla_components::EquipmentSlot::Body },
                        "mainhand" => quote! { vanilla_components::EquipmentSlot::MainHand },
                        "offhand" => quote! { vanilla_components::EquipmentSlot::OffHand },
                        "saddle" => quote! { vanilla_components::EquipmentSlot::Saddle },
                        _ => continue,
                    };
                    let allowed_entities = generate_allowed_entities(value);
                    let equip_sound = sound_event_holder_token(
                        value,
                        "equip_sound",
                        "minecraft:item.armor.equip_generic",
                    );
                    let asset_id = optional_identifier_token(value, "asset_id");
                    let camera_overlay = optional_identifier_token(value, "camera_overlay");
                    let dispensable = value
                        .get("dispensable")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true);
                    let swappable = value
                        .get("swappable")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true);
                    let damage_on_hurt = value
                        .get("damage_on_hurt")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true);
                    let equip_on_interact = value
                        .get("equip_on_interact")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let can_be_sheared = value
                        .get("can_be_sheared")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let shearing_sound = sound_event_holder_token(
                        value,
                        "shearing_sound",
                        "minecraft:item.shears.snip",
                    );
                    builder_calls.push(quote! {
                        .builder_set(
                            vanilla_components::EQUIPPABLE,
                            Some(vanilla_components::Equippable {
                                slot: #slot_variant,
                                equip_sound: #equip_sound,
                                asset_id: #asset_id,
                                camera_overlay: #camera_overlay,
                                allowed_entities: #allowed_entities,
                                dispensable: #dispensable,
                                swappable: #swappable,
                                damage_on_hurt: #damage_on_hurt,
                                equip_on_interact: #equip_on_interact,
                                can_be_sheared: #can_be_sheared,
                                shearing_sound: #shearing_sound,
                            }),
                        )
                    });
                }
            }
            "minecraft:tool" => {
                let tool_token = generate_tool_component(value);
                builder_calls
                    .push(quote! { .builder_set(vanilla_components::TOOL, Some(#tool_token)) });
            }
            "minecraft:attribute_modifiers" => {
                if let Some(modifiers) = generate_attribute_modifiers_component(value) {
                    builder_calls.push(quote! {
                        .builder_set(vanilla_components::ATTRIBUTE_MODIFIERS, Some(#modifiers))
                    });
                }
            }
            "minecraft:minimum_attack_charge" => {
                let val = value
                    .as_f64()
                    .expect("minimum_attack_charge component must be a number")
                    as f32;
                builder_calls.push(
                    quote! { .builder_set(vanilla_components::MINIMUM_ATTACK_CHARGE, Some(#val)) },
                );
            }
            "minecraft:damage_type" => {
                let damage_type = value
                    .as_str()
                    .expect("damage_type component must be an identifier string");
                let damage_type = damage_type_ref_token(damage_type);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::DAMAGE_TYPE,
                        Some(vanilla_components::DamageTypeComponent::new(#damage_type)),
                    )
                });
            }
            "minecraft:use_cooldown" => {
                let seconds = value
                    .get("seconds")
                    .and_then(Value::as_f64)
                    .expect("use_cooldown.seconds must be a number")
                    as f32;
                let cooldown_group = value
                    .get("cooldown_group")
                    .and_then(Value::as_str)
                    .map_or_else(
                        || quote! { None },
                        |group| {
                            let id = Identifier::from_str(group)
                                .expect("use_cooldown.cooldown_group must be an identifier");
                            let namespace = id.namespace.as_ref();
                            let path = id.path.as_ref();
                            quote! { Some(Identifier::new_static(#namespace, #path)) }
                        },
                    );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::USE_COOLDOWN,
                        Some(vanilla_components::UseCooldown::new(#seconds, #cooldown_group)),
                    )
                });
            }
            "minecraft:weapon" => {
                let weapon = generate_weapon_component(value);
                builder_calls
                    .push(quote! { .builder_set(vanilla_components::WEAPON, Some(#weapon)) });
            }
            "minecraft:attack_range" => {
                let attack_range = generate_attack_range_component(value);
                builder_calls.push(
                    quote! { .builder_set(vanilla_components::ATTACK_RANGE, Some(#attack_range)) },
                );
            }
            "minecraft:piercing_weapon" => {
                let piercing_weapon = generate_piercing_weapon_component(value);
                builder_calls.push(
                    quote! { .builder_set(vanilla_components::PIERCING_WEAPON, Some(#piercing_weapon)) },
                );
            }
            _ => {
                // TODO: Implement more
            }
        }
    }

    builder_calls
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/items.json");
    let item_assets: Items =
        serde_json::from_str(&fs::read_to_string("build_assets/items.json").unwrap()).unwrap();

    let mut item_definitions = TokenStream::new();
    let mut item_construction = TokenStream::new();

    let mut register_stream = TokenStream::new();
    for item in &item_assets.items {
        let item_ident = Ident::new(&item.name, Span::call_site());
        let item_name_str = item.name.clone();

        item_definitions.extend(quote! {
           pub #item_ident: Item,
        });

        if let Some(block_name) = &item.block_item {
            let block_ident = Ident::new(&block_name.to_shouty_snake_case(), Span::call_site());
            let builder_calls = generate_builder_calls(item);

            if builder_calls.is_empty() {
                if block_name == &item.name {
                    item_construction.extend(quote! {
                        #item_ident: Item::from_block(&vanilla_blocks::#block_ident),
                    });
                } else {
                    item_construction.extend(quote! {
                        #item_ident: Item::from_block_custom_name(&vanilla_blocks::#block_ident, #item_name_str),
                    });
                }
            } else {
                // Block item with custom components
                if block_name == &item.name {
                    item_construction.extend(quote! {
                        #item_ident: Item::from_block(&vanilla_blocks::#block_ident)
                            #(#builder_calls)*,
                    });
                } else {
                    item_construction.extend(quote! {
                        #item_ident: Item::from_block_custom_name(&vanilla_blocks::#block_ident, #item_name_str)
                            #(#builder_calls)*,
                    });
                }
            }
        } else {
            let builder_calls = generate_builder_calls(item);

            let craft_remainder_value = if let Some(remainder) = get_craft_remainder(&item.name) {
                quote! { Some(Identifier::vanilla_static(#remainder)) }
            } else {
                quote! { None }
            };

            item_construction.extend(quote! {
                #item_ident: Item {
                    key: Identifier::vanilla_static(#item_name_str),
                    components: DataComponentMap::common_item_components()
                        #(#builder_calls)*,
                    craft_remainder: #craft_remainder_value,
                    id: OnceLock::new(),
                },
            });
        }

        register_stream.extend(quote! {
            registry.register(&ITEMS.#item_ident);
        });
    }

    quote! {
        use crate::{
            data_components::{vanilla_components, DataComponentMap},
            vanilla_attributes, vanilla_blocks, vanilla_entities,
            items::{Item, ItemRegistry},
        };
        use steel_utils::Identifier;
        use std::sync::{LazyLock, OnceLock};

        pub static ITEMS: LazyLock<Items> = LazyLock::new(Items::init);

        pub struct Items {
            #item_definitions
        }

        impl Items {
            fn init() -> Self {
                Self {
                    #item_construction
                }
            }
        }

        pub fn register_items(registry: &mut ItemRegistry) {
            #register_stream
        }
    }
}
