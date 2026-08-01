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

mod attributes;
mod basic_components;
mod builder;
mod consumables;
mod kinetic;
mod metadata;
mod weapons;

use attributes::{generate_allowed_entities, generate_attribute_modifiers_component};
use basic_components::{
    block_state_component_token, blocks_attacks_component_token, fireworks_component_token,
    food_component_token,
};
use builder::{generate_builder_calls, get_craft_remainder};
use consumables::{consumable_component_token, death_protection_component_token};
use kinetic::kinetic_weapon_component_token;
use metadata::{
    banner_pattern_ref_token, damage_type_ref_token, entity_type_ref_token,
    holder_set_component_field, holder_set_token, item_name_component_token, item_ref_token,
    optional_identifier_token, rarity_component_token, registry_sound_event_holder_token,
    sound_event_holder_token, sound_event_value_token, swing_animation_component_token,
    use_effects_component_token,
};
use weapons::{
    generate_attack_range_component, generate_piercing_weapon_component, generate_tool_rule,
    generate_weapon_component,
};

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
    pub wall_block: Option<String>,
    #[serde(default)]
    pub is_double: bool,
    #[serde(default)]
    pub is_scaffolding: bool,
    #[serde(default)]
    pub is_water_placable: bool,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Items {
    pub items: Vec<Item>,
    pub block_item_mappings: BTreeMap<String, String>,
}

fn get_component_ident(name: &str) -> Ident {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let shouty_name = name.to_shouty_snake_case();
    Ident::new(&shouty_name, Span::call_site())
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
    let id =
        Identifier::from_str(s).unwrap_or_else(|error| panic!("invalid identifier {s:?}: {error}"));
    let namespace = id.namespace.as_ref();
    let path = id.path.as_ref();
    quote! { Identifier::new_static(#namespace, #path) }
}

fn jukebox_song_ref_token(value: &Value) -> TokenStream {
    let song = value
        .as_str()
        .unwrap_or_else(|| panic!("jukebox_playable component must be an identifier string"));
    let id = Identifier::from_str(song)
        .unwrap_or_else(|error| panic!("invalid jukebox song id {song:?}: {error}"));
    assert_eq!(
        id.namespace.as_ref(),
        "minecraft",
        "vanilla item prototype references a non-vanilla jukebox song: {id}"
    );
    let ident = if id
        .path
        .chars()
        .next()
        .is_some_and(|value| value.is_ascii_digit())
    {
        Ident::new(
            &format!("MUSIC_DISC_{}", id.path.to_shouty_snake_case()),
            Span::call_site(),
        )
    } else {
        Ident::new(&id.path.to_shouty_snake_case(), Span::call_site())
    };
    quote! { &vanilla_jukebox_songs::#ident }
}

fn instrument_ref_token(value: &Value) -> TokenStream {
    let instrument = value
        .as_str()
        .unwrap_or_else(|| panic!("instrument component must be an identifier string"));
    let id = Identifier::from_str(instrument)
        .unwrap_or_else(|error| panic!("invalid instrument id {instrument:?}: {error}"));
    assert_eq!(
        id.namespace.as_ref(),
        "minecraft",
        "vanilla item prototype references a non-vanilla instrument: {id}"
    );
    let ident = Ident::new(&id.path.to_shouty_snake_case(), Span::call_site());
    quote! { &vanilla_instruments::#ident }
}

fn trim_material_ref_token(value: &Value) -> TokenStream {
    let material = value
        .as_str()
        .unwrap_or_else(|| panic!("provides_trim_material component must be an identifier string"));
    let id = Identifier::from_str(material)
        .unwrap_or_else(|error| panic!("invalid trim material id {material:?}: {error}"));
    assert_eq!(
        id.namespace.as_ref(),
        "minecraft",
        "vanilla item prototype references a non-vanilla trim material: {id}"
    );
    let ident = Ident::new(&id.path.to_shouty_snake_case(), Span::call_site());
    quote! { &*crate::vanilla_trim_materials::#ident }
}

fn dye_color_token(value: &Value) -> TokenStream {
    let color = value
        .as_str()
        .unwrap_or_else(|| panic!("dye color component must be a string, got {value}"));
    let variant = match color {
        "white" => quote! { White },
        "orange" => quote! { Orange },
        "magenta" => quote! { Magenta },
        "light_blue" => quote! { LightBlue },
        "yellow" => quote! { Yellow },
        "lime" => quote! { Lime },
        "pink" => quote! { Pink },
        "gray" => quote! { Gray },
        "light_gray" => quote! { LightGray },
        "cyan" => quote! { Cyan },
        "purple" => quote! { Purple },
        "blue" => quote! { Blue },
        "brown" => quote! { Brown },
        "green" => quote! { Green },
        "red" => quote! { Red },
        "black" => quote! { Black },
        _ => panic!("unknown extracted dye color {color:?}"),
    };
    quote! { vanilla_components::DyeColor::#variant }
}

fn component_i32(value: &Value, component: &str) -> i32 {
    let value = value
        .as_i64()
        .unwrap_or_else(|| panic!("{component} component must be an integer"));
    i32::try_from(value).unwrap_or_else(|_| panic!("{component} component must fit an i32"))
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/items.json");
    let item_assets: Items =
        serde_json::from_str(&fs::read_to_string("build_assets/items.json").unwrap()).unwrap();

    let mut item_statics = TokenStream::new();

    let mut register_stream = TokenStream::new();
    for item in &item_assets.items {
        let item_ident = Ident::new(&item.name.to_shouty_snake_case(), Span::call_site());
        let item_name_str = item.name.clone();
        let item_name = item.components.get("minecraft:item_name").map_or_else(
            || panic!("item {} is missing its item_name component", item.name),
            item_name_component_token,
        );

        if let Some(block_name) = &item.block_item {
            let block_ident = Ident::new(&block_name.to_shouty_snake_case(), Span::call_site());
            let builder_calls = generate_builder_calls(item);

            if block_name == &item.name {
                item_statics.extend(quote! {
                    pub static #item_ident: LazyLock<Item> = LazyLock::new(|| {
                        Item::from_block(
                            &vanilla_blocks::#block_ident,
                            #item_name,
                        )
                            #(#builder_calls)*
                    });
                });
            } else {
                item_statics.extend(quote! {
                    pub static #item_ident: LazyLock<Item> = LazyLock::new(|| {
                        Item::from_block_custom_name(
                            &vanilla_blocks::#block_ident,
                            #item_name_str,
                            #item_name,
                        )
                            #(#builder_calls)*
                    });
                });
            }
        } else {
            let builder_calls = generate_builder_calls(item);

            let craft_remainder_value = if let Some(remainder) = get_craft_remainder(&item.name) {
                quote! { Some(Identifier::vanilla_static(#remainder)) }
            } else {
                quote! { None }
            };

            item_statics.extend(quote! {
                pub static #item_ident: LazyLock<Item> = LazyLock::new(|| {
                    Item::new(
                        Identifier::vanilla_static(#item_name_str),
                        #item_name,
                        #craft_remainder_value,
                    )
                        #(#builder_calls)*
                });
            });
        }

        register_stream.extend(quote! {
            registry.register(&#item_ident);
        });
    }

    for (block_name, item_name) in &item_assets.block_item_mappings {
        let block_ident = Ident::new(&block_name.to_shouty_snake_case(), Span::call_site());
        let item_ident = Ident::new(&item_name.to_shouty_snake_case(), Span::call_site());
        register_stream.extend(quote! {
            registry.register_block_item(&vanilla_blocks::#block_ident, &#item_ident);
        });
    }

    quote! {
        use crate::{
            data_components::vanilla_components,
            vanilla_attributes, vanilla_blocks, vanilla_entities, vanilla_instruments,
            vanilla_jukebox_songs,
            items::{Item, ItemRegistry},
        };
        use steel_utils::Identifier;
        use std::{collections::BTreeMap, sync::LazyLock};
        use text_components::{TextComponent, translation::TranslatedMessage};

        #item_statics

        pub fn register_items(registry: &mut ItemRegistry) {
            #register_stream
        }
    }
}
