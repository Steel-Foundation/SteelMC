use std::{collections::BTreeMap, fs};

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
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

/// Generates the TokenStream for a Tool component from JSON data.
fn generate_tool_component(value: &Value) -> TokenStream {
    let rules = value
        .get("rules")
        .and_then(|r| r.as_array())
        .map(|rules_arr| rules_arr.iter().map(generate_tool_rule).collect::<Vec<_>>())
        .unwrap_or_default();

    let default_mining_speed = value
        .get("default_mining_speed")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0) as f32;

    let damage_per_block = value
        .get("damage_per_block")
        .and_then(|v| v.as_i64())
        .unwrap_or(1) as i32;

    let can_destroy_blocks_in_creative = value
        .get("can_destroy_blocks_in_creative")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    quote! {
        vanilla_components::Tool {
            rules: vec![#(#rules),*],
            default_mining_speed: #default_mining_speed,
            damage_per_block: #damage_per_block,
            can_destroy_blocks_in_creative: #can_destroy_blocks_in_creative,
        }
    }
}

/// Parses a block or tag reference string into an Identifier TokenStream.
/// For tags like "#minecraft:mineable/pickaxe", creates Identifier { namespace: "#minecraft", path: "mineable/pickaxe" }
/// For blocks like "minecraft:stone", creates Identifier { namespace: "minecraft", path: "stone" }
fn parse_block_or_tag(s: &str) -> TokenStream {
    let (is_tag, rest) = if let Some(stripped) = s.strip_prefix('#') {
        (true, stripped)
    } else {
        (false, s)
    };

    // Split namespace:path
    let parts: Vec<&str> = rest.splitn(2, ':').collect();
    let (namespace, path) = if parts.len() == 2 {
        (parts[0], parts[1])
    } else {
        // Default to minecraft namespace
        ("minecraft", rest)
    };

    if is_tag {
        // Prefix namespace with # for tags
        let tag_namespace = format!("#{namespace}");
        quote! { Identifier::new(#tag_namespace, #path) }
    } else {
        quote! { Identifier::new(#namespace, #path) }
    }
}

/// Generates the TokenStream for a single ToolRule from JSON data.
fn generate_tool_rule(rule: &Value) -> TokenStream {
    // Parse blocks - can be a string (single block or tag), or an array of strings
    let blocks_value = rule.get("blocks");
    let blocks_tokens: Vec<TokenStream> = match blocks_value {
        Some(Value::String(s)) => {
            vec![parse_block_or_tag(s)]
        }
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .map(parse_block_or_tag)
            .collect(),
        _ => vec![],
    };

    // Parse optional speed
    let speed_token = match rule.get("speed").and_then(|v| v.as_f64()) {
        Some(speed) => {
            let speed = speed as f32;
            quote! { Some(#speed) }
        }
        None => quote! { None },
    };

    // Parse optional correct_for_drops
    let correct_for_drops_token = match rule.get("correct_for_drops").and_then(|v| v.as_bool()) {
        Some(correct) => quote! { Some(#correct) },
        None => quote! { None },
    };

    quote! {
        vanilla_components::ToolRule {
            blocks: vec![#(#blocks_tokens),*],
            speed: #speed_token,
            correct_for_drops: #correct_for_drops_token,
        }
    }
}

/// Returns the crafting remainder item key for a given item, if any.
/// Based on vanilla Minecraft's Item.Properties.craftRemainder() calls.
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
                        "head" => quote! { vanilla_components::EquippableSlot::Head },
                        "chest" => quote! { vanilla_components::EquippableSlot::Chest },
                        "legs" => quote! { vanilla_components::EquippableSlot::Legs },
                        "feet" => quote! { vanilla_components::EquippableSlot::Feet },
                        "body" => quote! { vanilla_components::EquippableSlot::Body },
                        "mainhand" => quote! { vanilla_components::EquippableSlot::Mainhand },
                        "offhand" => quote! { vanilla_components::EquippableSlot::Offhand },
                        "saddle" => quote! { vanilla_components::EquippableSlot::Saddle },
                        _ => continue,
                    };
                    builder_calls.push(
                        quote! { .builder_set(vanilla_components::EQUIPPABLE, Some(vanilla_components::Equippable { slot: #slot_variant })) },
                    );
                }
            }
            "minecraft:tool" => {
                let tool_token = generate_tool_component(value);
                builder_calls
                    .push(quote! { .builder_set(vanilla_components::TOOL, Some(#tool_token)) });
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
                if block_name != &item.name {
                    item_construction.extend(quote! {
                        #item_ident: Item::from_block_custom_name(vanilla_blocks::#block_ident, #item_name_str),
                    });
                } else {
                    item_construction.extend(quote! {
                        #item_ident: Item::from_block(vanilla_blocks::#block_ident),
                    });
                }
            } else {
                // Block item with custom components
                if block_name != &item.name {
                    item_construction.extend(quote! {
                        #item_ident: Item::from_block_custom_name(vanilla_blocks::#block_ident, #item_name_str)
                            #(#builder_calls)*,
                    });
                } else {
                    item_construction.extend(quote! {
                        #item_ident: Item::from_block(vanilla_blocks::#block_ident)
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
                },
            });
        }
    }

    let mut register_stream = TokenStream::new();
    for item in &item_assets.items {
        let item_name = Ident::new(&item.name, Span::call_site());
        register_stream.extend(quote! {
            registry.register(&ITEMS.#item_name);
        });
    }

    quote! {
        use crate::{
            data_components::{vanilla_components, DataComponentMap},
            vanilla_blocks,
            items::{Item, ItemRegistry},
        };
        use steel_utils::Identifier;
        use std::sync::LazyLock;

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
