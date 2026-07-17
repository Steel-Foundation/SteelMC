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

fn food_component_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("food component must be an object"));
    let nutrition = object
        .get("nutrition")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("food.nutrition must be an integer"));
    let nutrition =
        i32::try_from(nutrition).unwrap_or_else(|_| panic!("food.nutrition must fit an i32"));
    assert!(nutrition >= 0, "food.nutrition must be non-negative");
    let saturation = object
        .get("saturation")
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("food.saturation must be a number")) as f32;
    let can_always_eat = object.get("can_always_eat").is_some_and(|value| {
        value
            .as_bool()
            .unwrap_or_else(|| panic!("food.can_always_eat must be a boolean"))
    });
    quote! {
        vanilla_components::FoodProperties::from_extracted(
            #nutrition,
            #saturation,
            #can_always_eat,
        )
    }
}

fn block_state_component_token(value: &Value) -> TokenStream {
    let properties = value
        .as_object()
        .unwrap_or_else(|| panic!("block_state component must be an object"))
        .iter()
        .map(|(name, value)| {
            let value = value
                .as_str()
                .unwrap_or_else(|| panic!("block_state.{name} must be a string"));
            quote! { (#name.to_owned(), #value.to_owned()) }
        });
    quote! {
        vanilla_components::BlockItemStateProperties::new(
            BTreeMap::from([#(#properties),*]),
        )
    }
}

fn fireworks_component_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("fireworks component must be an object"));
    let flight_duration = object.get("flight_duration").map_or(0, |value| {
        let value = value
            .as_i64()
            .unwrap_or_else(|| panic!("fireworks.flight_duration must be an integer"));
        i32::try_from(value).unwrap_or_else(|_| panic!("fireworks.flight_duration must fit an i32"))
    });
    assert!(
        (0..=u8::MAX.into()).contains(&flight_duration),
        "fireworks.flight_duration must be in 0..=255"
    );
    assert!(
        object
            .get("explosions")
            .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty)),
        "vanilla item prototypes currently require empty firework explosions"
    );
    quote! { vanilla_components::Fireworks::from_extracted(#flight_duration) }
}

fn blocks_attacks_component_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("blocks_attacks component must be an object"));
    assert!(
        object.keys().all(|key| matches!(
            key.as_str(),
            "block_delay_seconds"
                | "item_damage"
                | "bypassed_by"
                | "block_sound"
                | "disabled_sound"
        )),
        "shield blocks_attacks contains unsupported fields: {value}"
    );
    let block_delay_seconds = object
        .get("block_delay_seconds")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    assert!(
        block_delay_seconds >= 0.0 && !block_delay_seconds.is_nan(),
        "shield block delay must be non-negative"
    );
    let item_damage = object
        .get("item_damage")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("shield blocks_attacks must define item_damage"));
    let item_damage_value = |field: &str| {
        item_damage
            .get(field)
            .and_then(Value::as_f64)
            .unwrap_or_else(|| panic!("blocks_attacks.item_damage.{field} must be a number"))
            as f32
    };
    let threshold = item_damage_value("threshold");
    let base = item_damage_value("base");
    let factor = item_damage_value("factor");
    assert!(
        threshold >= 0.0,
        "item damage threshold must be non-negative"
    );
    let bypassed_by = object
        .get("bypassed_by")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("shield blocks_attacks must define bypassed_by"));
    let bypassed_by = bypassed_by
        .strip_prefix('#')
        .unwrap_or_else(|| panic!("shield bypassed_by must be a damage-type tag"));
    let bypassed_by = identifier_token(bypassed_by);
    let block_sound = sound_event_value_token(
        object
            .get("block_sound")
            .unwrap_or_else(|| panic!("shield blocks_attacks must define block_sound")),
        "blocks_attacks.block_sound",
    );
    let disabled_sound = sound_event_value_token(
        object
            .get("disabled_sound")
            .unwrap_or_else(|| panic!("shield blocks_attacks must define disabled_sound")),
        "blocks_attacks.disabled_sound",
    );
    quote! {
        vanilla_components::BlocksAttacks::from_extracted_shield(
            #block_delay_seconds,
            vanilla_components::ItemDamageFunction::from_extracted(#threshold, #base, #factor),
            crate::RegistryHolderSet::Tag(#bypassed_by),
            #block_sound,
            #disabled_sound,
        )
    }
}

fn mob_effect_ref_token(value: &str) -> TokenStream {
    let id = Identifier::from_str(value)
        .unwrap_or_else(|error| panic!("invalid mob effect id {value:?}: {error}"));
    assert_eq!(
        id.namespace.as_ref(),
        "minecraft",
        "vanilla item prototypes must reference vanilla mob effects: {id}"
    );
    let ident = Ident::new(&id.path.to_shouty_snake_case(), Span::call_site());
    quote! { &crate::vanilla_mob_effects::#ident }
}

fn mob_effect_instance_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("mob effect instance must be an object"));
    assert!(
        object.keys().all(|key| matches!(
            key.as_str(),
            "id" | "duration" | "amplifier" | "ambient" | "show_particles" | "show_icon"
        )),
        "extracted mob effect instance contains unsupported fields: {value}"
    );
    let effect = object.get("id").and_then(Value::as_str).map_or_else(
        || panic!("mob effect instance must define id"),
        mob_effect_ref_token,
    );
    let integer = |field: &str, default: i32| {
        object.get(field).map_or(default, |value| {
            let value = value
                .as_i64()
                .unwrap_or_else(|| panic!("mob effect {field} must be an integer"));
            i32::try_from(value).unwrap_or_else(|_| panic!("mob effect {field} must fit an i32"))
        })
    };
    let boolean = |field: &str, default: bool| {
        object.get(field).map_or(default, |value| {
            value
                .as_bool()
                .unwrap_or_else(|| panic!("mob effect {field} must be a boolean"))
        })
    };
    let duration = integer("duration", 0);
    let amplifier = integer("amplifier", 0);
    let ambient = boolean("ambient", false);
    let show_particles = boolean("show_particles", true);
    let show_icon = boolean("show_icon", show_particles);
    quote! {
        crate::MobEffectInstance::new(
            #effect,
            #duration,
            #amplifier,
            #ambient,
            #show_particles,
            #show_icon,
            None,
        )
    }
}

fn consume_effect_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("consume effect must be an object"));
    let effect_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("consume effect must define type"));
    match effect_type {
        "minecraft:apply_effects" => {
            let effects = object
                .get("effects")
                .and_then(Value::as_array)
                .unwrap_or_else(|| panic!("apply_effects must define an effects array"))
                .iter()
                .map(mob_effect_instance_token);
            let probability = object.get("probability").map_or(1.0, |value| {
                value
                    .as_f64()
                    .unwrap_or_else(|| panic!("apply_effects probability must be a number"))
                    as f32
            });
            assert!(
                !probability.is_nan() && (0.0..=1.0).contains(&probability),
                "apply_effects probability must be in 0..=1"
            );
            quote! {
                crate::ConsumeEffectData::new(
                    &crate::consume_effect::vanilla_consume_effect_types::APPLY_EFFECTS,
                    crate::consume_effect::ApplyStatusEffectsConsumeEffect::from_extracted(
                        vec![#(#effects),*],
                        #probability,
                    ),
                )
            }
        }
        "minecraft:remove_effects" => {
            let effects = object
                .get("effects")
                .unwrap_or_else(|| panic!("remove_effects must define effects"));
            let effects = holder_set_token(effects, "remove_effects", mob_effect_ref_token);
            quote! {
                crate::ConsumeEffectData::new(
                    &crate::consume_effect::vanilla_consume_effect_types::REMOVE_EFFECTS,
                    crate::consume_effect::RemoveStatusEffectsConsumeEffect::new(#effects),
                )
            }
        }
        "minecraft:clear_all_effects" => quote! {
            crate::ConsumeEffectData::new(
                &crate::consume_effect::vanilla_consume_effect_types::CLEAR_ALL_EFFECTS,
                crate::consume_effect::ClearAllStatusEffectsConsumeEffect,
            )
        },
        "minecraft:teleport_randomly" => {
            let diameter = object.get("diameter").map_or(16.0, |value| {
                value
                    .as_f64()
                    .unwrap_or_else(|| panic!("teleport_randomly diameter must be a number"))
                    as f32
            });
            assert!(
                diameter > 0.0,
                "teleport_randomly diameter must be positive"
            );
            quote! {
                crate::ConsumeEffectData::new(
                    &crate::consume_effect::vanilla_consume_effect_types::TELEPORT_RANDOMLY,
                    crate::consume_effect::TeleportRandomlyConsumeEffect::from_extracted(#diameter),
                )
            }
        }
        "minecraft:play_sound" => {
            let sound = object
                .get("sound")
                .unwrap_or_else(|| panic!("play_sound must define sound"));
            let sound = sound_event_value_token(sound, "consume_effect.play_sound");
            quote! {
                crate::ConsumeEffectData::new(
                    &crate::consume_effect::vanilla_consume_effect_types::PLAY_SOUND,
                    crate::consume_effect::PlaySoundConsumeEffect::new(#sound),
                )
            }
        }
        _ => panic!("unknown extracted consume effect type {effect_type:?}"),
    }
}

fn consume_effects_token(value: Option<&Value>, field: &str) -> Vec<TokenStream> {
    value.map_or_else(Vec::new, |value| {
        value
            .as_array()
            .unwrap_or_else(|| panic!("{field} must be an array"))
            .iter()
            .map(consume_effect_token)
            .collect()
    })
}

fn item_use_animation_token(value: Option<&Value>) -> TokenStream {
    let name = value.map_or("eat", |value| {
        value
            .as_str()
            .unwrap_or_else(|| panic!("consumable animation must be a string"))
    });
    let variant = match name {
        "none" => quote! { None },
        "eat" => quote! { Eat },
        "drink" => quote! { Drink },
        "block" => quote! { Block },
        "bow" => quote! { Bow },
        "trident" => quote! { Trident },
        "crossbow" => quote! { Crossbow },
        "spyglass" => quote! { Spyglass },
        "toot_horn" => quote! { TootHorn },
        "brush" => quote! { Brush },
        "bundle" => quote! { Bundle },
        "spear" => quote! { Spear },
        _ => panic!("unknown consumable animation {name:?}"),
    };
    quote! { vanilla_components::ItemUseAnimation::#variant }
}

fn consumable_component_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("consumable component must be an object"));
    assert!(
        object.keys().all(|key| matches!(
            key.as_str(),
            "consume_seconds"
                | "animation"
                | "sound"
                | "has_consume_particles"
                | "on_consume_effects"
        )),
        "consumable component contains unsupported fields: {value}"
    );
    let consume_seconds = object.get("consume_seconds").map_or(1.6, |value| {
        value
            .as_f64()
            .unwrap_or_else(|| panic!("consume_seconds must be a number")) as f32
    });
    assert!(
        consume_seconds >= 0.0 && !consume_seconds.is_nan(),
        "consume_seconds must be non-negative"
    );
    let animation = item_use_animation_token(object.get("animation"));
    let sound = object.get("sound").map_or_else(
        || registry_sound_event_holder_token("minecraft:entity.generic.eat", "consumable.sound"),
        |value| sound_event_value_token(value, "consumable.sound"),
    );
    let has_consume_particles = object.get("has_consume_particles").is_none_or(|value| {
        value
            .as_bool()
            .unwrap_or_else(|| panic!("has_consume_particles must be a boolean"))
    });
    let effects = consume_effects_token(object.get("on_consume_effects"), "on_consume_effects");
    quote! {
        vanilla_components::Consumable::from_extracted(
            #consume_seconds,
            #animation,
            #sound,
            #has_consume_particles,
            vec![#(#effects),*],
        )
    }
}

fn death_protection_component_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("death_protection component must be an object"));
    assert!(
        object.keys().all(|key| key == "death_effects"),
        "death_protection component contains unsupported fields: {value}"
    );
    let effects = consume_effects_token(object.get("death_effects"), "death_effects");
    quote! { vanilla_components::DeathProtection::new(vec![#(#effects),*]) }
}

fn kinetic_condition_token(value: Option<&Value>, field: &str) -> TokenStream {
    let Some(value) = value else {
        return quote! { None };
    };
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("kinetic_weapon.{field} must be an object"));
    let max_duration_ticks = object
        .get("max_duration_ticks")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("kinetic_weapon.{field}.max_duration_ticks must be an integer"));
    let max_duration_ticks = i32::try_from(max_duration_ticks)
        .unwrap_or_else(|_| panic!("kinetic_weapon.{field}.max_duration_ticks must fit an i32"));
    assert!(
        max_duration_ticks >= 0,
        "kinetic_weapon.{field}.max_duration_ticks must be non-negative"
    );
    let min_speed = object.get("min_speed").map_or(0.0, |value| {
        value
            .as_f64()
            .unwrap_or_else(|| panic!("kinetic_weapon.{field}.min_speed must be a number"))
            as f32
    });
    let min_relative_speed = object.get("min_relative_speed").map_or(0.0, |value| {
        value
            .as_f64()
            .unwrap_or_else(|| panic!("kinetic_weapon.{field}.min_relative_speed must be a number"))
            as f32
    });
    quote! {
        Some(vanilla_components::KineticWeaponCondition::from_extracted(
            #max_duration_ticks,
            #min_speed,
            #min_relative_speed,
        ))
    }
}

fn kinetic_weapon_component_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("kinetic_weapon component must be an object"));
    let non_negative_i32 = |field: &str, default: i32| {
        let value = object.get(field).map_or(i64::from(default), |value| {
            value
                .as_i64()
                .unwrap_or_else(|| panic!("kinetic_weapon.{field} must be an integer"))
        });
        let value = i32::try_from(value)
            .unwrap_or_else(|_| panic!("kinetic_weapon.{field} must fit an i32"));
        assert!(value >= 0, "kinetic_weapon.{field} must be non-negative");
        value
    };
    let float = |field: &str, default: f32| {
        object.get(field).map_or(default, |value| {
            value
                .as_f64()
                .unwrap_or_else(|| panic!("kinetic_weapon.{field} must be a number"))
                as f32
        })
    };
    let contact_cooldown_ticks = non_negative_i32("contact_cooldown_ticks", 10);
    let delay_ticks = non_negative_i32("delay_ticks", 0);
    let dismount_conditions =
        kinetic_condition_token(object.get("dismount_conditions"), "dismount_conditions");
    let knockback_conditions =
        kinetic_condition_token(object.get("knockback_conditions"), "knockback_conditions");
    let damage_conditions =
        kinetic_condition_token(object.get("damage_conditions"), "damage_conditions");
    let forward_movement = float("forward_movement", 0.0);
    let damage_multiplier = float("damage_multiplier", 1.0);
    let sound = object.get("sound").map_or_else(
        || quote! { None },
        |sound| {
            let sound = sound_event_value_token(sound, "kinetic_weapon.sound");
            quote! { Some(#sound) }
        },
    );
    let hit_sound = object.get("hit_sound").map_or_else(
        || quote! { None },
        |sound| {
            let sound = sound_event_value_token(sound, "kinetic_weapon.hit_sound");
            quote! { Some(#sound) }
        },
    );
    quote! {
        vanilla_components::KineticWeapon::from_extracted(
            #contact_cooldown_ticks,
            #delay_ticks,
            #dismount_conditions,
            #knockback_conditions,
            #damage_conditions,
            #forward_movement,
            #damage_multiplier,
            #sound,
            #hit_sound,
        )
    }
}

fn item_name_component_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("item_name component must be an object"));
    assert_eq!(
        object.len(),
        1,
        "vanilla item_name component contains unsupported fields: {value}"
    );
    let translation = object
        .get("translate")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("item_name component must contain a translate string"));
    quote! {
        TextComponent::translated(TranslatedMessage::new(#translation, None))
    }
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
        panic!("invalid sound event id {sound:?} in item component field {field}: {error}")
    });
    let sound = generate_sound_event_ref(&id);
    quote! { crate::sound_event::SoundEventHolder::registry(#sound) }
}

fn sound_event_value_token(value: &Value, field: &str) -> TokenStream {
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

fn sound_event_holder_token(value: &Value, field: &str, default: &str) -> TokenStream {
    value.get(field).map_or_else(
        || registry_sound_event_holder_token(default, field),
        |value| sound_event_value_token(value, field),
    )
}

fn rarity_component_token(value: &Value) -> Option<TokenStream> {
    match value
        .as_str()
        .unwrap_or_else(|| panic!("rarity component must be a string"))
    {
        "common" => None,
        "uncommon" => Some(quote! { vanilla_components::Rarity::Uncommon }),
        "rare" => Some(quote! { vanilla_components::Rarity::Rare }),
        "epic" => Some(quote! { vanilla_components::Rarity::Epic }),
        rarity => panic!("unknown rarity component value: {rarity}"),
    }
}

fn use_effects_component_token(value: &Value) -> Option<TokenStream> {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("use_effects component must be an object"));
    assert!(
        object.keys().all(|key| matches!(
            key.as_str(),
            "can_sprint" | "interact_vibrations" | "speed_multiplier"
        )),
        "use_effects component contains an unknown field: {value}"
    );
    let can_sprint = object.get("can_sprint").is_some_and(|value| {
        value
            .as_bool()
            .expect("use_effects.can_sprint must be a boolean")
    });
    let interact_vibrations = object.get("interact_vibrations").is_none_or(|value| {
        value
            .as_bool()
            .expect("use_effects.interact_vibrations must be a boolean")
    });
    let speed_multiplier = object.get("speed_multiplier").map_or(0.2_f32, |value| {
        value
            .as_f64()
            .expect("use_effects.speed_multiplier must be a number") as f32
    });
    assert!(
        speed_multiplier.is_finite() && (0.0..=1.0).contains(&speed_multiplier),
        "use_effects.speed_multiplier must be between 0 and 1"
    );
    if !can_sprint && interact_vibrations && speed_multiplier.to_bits() == 0.2_f32.to_bits() {
        return None;
    }
    Some(quote! {
        vanilla_components::UseEffects::new(
            #can_sprint,
            #interact_vibrations,
            #speed_multiplier,
        )
    })
}

fn swing_animation_component_token(value: &Value) -> Option<TokenStream> {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("swing_animation component must be an object"));
    assert!(
        object
            .keys()
            .all(|key| matches!(key.as_str(), "type" | "duration")),
        "swing_animation component contains an unknown field: {value}"
    );
    let animation_type = match object.get("type").map_or("whack", |value| {
        value
            .as_str()
            .expect("swing_animation.type must be a string")
    }) {
        "none" => quote! { vanilla_components::SwingAnimationType::None },
        "whack" => quote! { vanilla_components::SwingAnimationType::Whack },
        "stab" => quote! { vanilla_components::SwingAnimationType::Stab },
        animation_type => panic!("unknown swing_animation type: {animation_type}"),
    };
    let duration = object.get("duration").map_or(6_i32, |value| {
        let duration = value
            .as_i64()
            .expect("swing_animation.duration must be an integer");
        i32::try_from(duration).expect("swing_animation.duration is outside the i32 range")
    });
    assert!(duration > 0, "swing_animation.duration must be positive");
    if object.is_empty() {
        return None;
    }
    Some(quote! {
        vanilla_components::SwingAnimation::new(#animation_type, #duration)
    })
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

fn banner_pattern_ref_token(value: &str) -> TokenStream {
    let id = Identifier::from_str(value)
        .unwrap_or_else(|error| panic!("invalid banner pattern id {value:?}: {error}"));
    assert_eq!(
        id.namespace.as_ref(),
        "minecraft",
        "vanilla item banner patterns must use the minecraft namespace: {id}"
    );

    let ident = Ident::new(&id.path.to_shouty_snake_case(), Span::call_site());
    quote! { &crate::vanilla_banner_patterns::#ident }
}

fn item_ref_token(value: &str, component: &str) -> TokenStream {
    let id = Identifier::from_str(value)
        .unwrap_or_else(|error| panic!("invalid {component} item id {value:?}: {error}"));
    assert_eq!(
        id.namespace.as_ref(),
        "minecraft",
        "vanilla {component} references must use the minecraft namespace: {id}"
    );
    let ident = Ident::new(&id.path.to_shouty_snake_case(), Span::call_site());
    quote! { &*#ident }
}

fn holder_set_token(
    value: &Value,
    component: &str,
    direct_ref: impl Fn(&str) -> TokenStream,
) -> TokenStream {
    match value {
        Value::String(value) if value.starts_with('#') => {
            let tag = value.trim_start_matches('#');
            Identifier::from_str(tag)
                .unwrap_or_else(|error| panic!("invalid {component} tag {value:?}: {error}"));
            let tag = identifier_token(tag);
            quote! { crate::RegistryHolderSet::Tag(#tag) }
        }
        Value::String(value) => {
            let entry = direct_ref(value);
            quote! { crate::RegistryHolderSet::Direct(vec![#entry]) }
        }
        Value::Array(values) => {
            let entries = values
                .iter()
                .map(|value| {
                    let value = value.as_str().unwrap_or_else(|| {
                        panic!("{component} direct holder list entries must be strings")
                    });
                    assert!(
                        !value.starts_with('#'),
                        "{component} direct holder lists cannot contain tags: {value}"
                    );
                    direct_ref(value)
                })
                .collect::<Vec<_>>();
            quote! { crate::RegistryHolderSet::Direct(vec![#(#entries),*]) }
        }
        _ => panic!("{component} holder set must be a string or string array"),
    }
}

fn holder_set_component_field<'a>(value: &'a Value, component: &str, field: &str) -> &'a Value {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("{component} component must be an object"));
    assert_eq!(
        object.len(),
        1,
        "{component} component must contain only {field}"
    );
    object
        .get(field)
        .unwrap_or_else(|| panic!("{component} component must contain {field}"))
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
            quote! { vanilla_components::ToolRuleBlocks::Direct(vec![#block]) }
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
            quote! { vanilla_components::ToolRuleBlocks::Direct(vec![#(#blocks),*]) }
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
        let component_ident = get_component_ident(key);

        match key.as_str() {
            "minecraft:item_name" => {
                item_name_component_token(value);
            }
            "minecraft:item_model" => {
                let model = value
                    .as_str()
                    .unwrap_or_else(|| panic!("item_model component must be an identifier"));
                let model = Identifier::from_str(model)
                    .unwrap_or_else(|error| panic!("invalid item_model {model:?}: {error}"));
                assert_eq!(
                    model,
                    Identifier::vanilla(item.name.clone()),
                    "vanilla 26.2 item model must default to its item key"
                );
            }
            "minecraft:bucket_entity_data" => {
                assert!(
                    value.as_object().is_some_and(serde_json::Map::is_empty),
                    "vanilla bucket_entity_data item prototype must be empty, got {value}"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::BUCKET_ENTITY_DATA,
                        Some(vanilla_components::CustomData::default()),
                    )
                });
            }
            "minecraft:entity_data" => {
                let object = value
                    .as_object()
                    .unwrap_or_else(|| panic!("entity_data component must be an object"));
                assert_eq!(
                    object.len(),
                    1,
                    "extracted entity_data prototypes currently require an id-only value"
                );
                let entity_type = object
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("entity_data.id must be an entity type identifier"));
                let entity_type = entity_type_ref_token(entity_type).unwrap_or_else(|| {
                    panic!("vanilla entity_data references non-vanilla type {entity_type}")
                });
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::ENTITY_DATA,
                        Some(vanilla_components::EntityData::new(
                            #entity_type,
                            vanilla_components::CustomData::default(),
                        )),
                    )
                });
            }
            "minecraft:debug_stick_state" => {
                assert!(
                    value.as_object().is_some_and(serde_json::Map::is_empty),
                    "vanilla debug stick prototype must have an empty state"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::DEBUG_STICK_STATE,
                        Some(vanilla_components::DebugStickState::empty()),
                    )
                });
            }
            "minecraft:writable_book_content" => {
                assert!(
                    value.as_object().is_some_and(serde_json::Map::is_empty),
                    "vanilla writable book prototype must have empty content"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::WRITABLE_BOOK_CONTENT,
                        Some(vanilla_components::WritableBookContent::empty()),
                    )
                });
            }
            "minecraft:suspicious_stew_effects" => {
                assert!(
                    value.as_array().is_some_and(Vec::is_empty),
                    "vanilla suspicious stew prototype must have empty effects"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::SUSPICIOUS_STEW_EFFECTS,
                        Some(vanilla_components::SuspiciousStewEffects::empty()),
                    )
                });
            }
            "minecraft:potion_contents" => {
                assert!(
                    value.as_object().is_some_and(serde_json::Map::is_empty),
                    "vanilla potion item prototypes must have empty potion contents"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::POTION_CONTENTS,
                        Some(vanilla_components::PotionContents::empty()),
                    )
                });
            }
            "minecraft:potion_duration_scale" => {
                let scale = value
                    .as_f64()
                    .unwrap_or_else(|| panic!("potion_duration_scale component must be a number"))
                    as f32;
                assert!(
                    scale.is_finite() && !scale.is_sign_negative(),
                    "potion_duration_scale must be non-negative and finite"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::POTION_DURATION_SCALE,
                        Some(#scale),
                    )
                });
            }
            "minecraft:food" => {
                let food = food_component_token(value);
                builder_calls.push(quote! { .builder_set(vanilla_components::FOOD, Some(#food)) });
            }
            "minecraft:fireworks" => {
                let fireworks = fireworks_component_token(value);
                builder_calls
                    .push(quote! { .builder_set(vanilla_components::FIREWORKS, Some(#fireworks)) });
            }
            "minecraft:block_state" => {
                let block_state = block_state_component_token(value);
                builder_calls.push(
                    quote! { .builder_set(vanilla_components::BLOCK_STATE, Some(#block_state)) },
                );
            }
            "minecraft:blocks_attacks" => {
                let blocks_attacks = blocks_attacks_component_token(value);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::BLOCKS_ATTACKS,
                        Some(#blocks_attacks),
                    )
                });
            }
            "minecraft:consumable" => {
                let consumable = consumable_component_token(value);
                builder_calls.push(
                    quote! { .builder_set(vanilla_components::CONSUMABLE, Some(#consumable)) },
                );
            }
            "minecraft:death_protection" => {
                let death_protection = death_protection_component_token(value);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::DEATH_PROTECTION,
                        Some(#death_protection),
                    )
                });
            }
            "minecraft:bees" => {
                assert!(
                    value.as_array().is_some_and(Vec::is_empty),
                    "vanilla beehive item prototypes currently require empty bee occupants"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::BEES,
                        Some(vanilla_components::Bees::empty()),
                    )
                });
            }
            "minecraft:chicken/variant" => {
                let variant = value.as_str().unwrap_or_else(|| {
                    panic!("chicken/variant component must be an identifier string")
                });
                let variant = Identifier::from_str(variant)
                    .unwrap_or_else(|error| panic!("invalid chicken variant {variant:?}: {error}"));
                assert_eq!(
                    variant.namespace.as_ref(),
                    "minecraft",
                    "vanilla item prototype references non-vanilla chicken variant {variant}"
                );
                let variant = Ident::new(&variant.path.to_shouty_snake_case(), Span::call_site());
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::CHICKEN_VARIANT,
                        Some(vanilla_components::RegistryReference::new(
                            &crate::vanilla_chicken_variants::#variant,
                        )),
                    )
                });
            }
            "minecraft:kinetic_weapon" => {
                let kinetic_weapon = kinetic_weapon_component_token(value);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::KINETIC_WEAPON,
                        Some(#kinetic_weapon),
                    )
                });
            }
            "minecraft:map_decorations" => {
                assert!(
                    value.as_object().is_some_and(serde_json::Map::is_empty),
                    "vanilla filled map prototype must have empty map decorations"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::MAP_DECORATIONS,
                        Some(vanilla_components::MapDecorations::EMPTY),
                    )
                });
            }
            "minecraft:enchantable" => {
                let object = value
                    .as_object()
                    .unwrap_or_else(|| panic!("enchantable component must be an object"));
                assert_eq!(
                    object.len(),
                    1,
                    "enchantable component must contain only its value"
                );
                let value = object
                    .get("value")
                    .and_then(Value::as_i64)
                    .unwrap_or_else(|| panic!("enchantable.value must be an integer"));
                let value = i32::try_from(value)
                    .unwrap_or_else(|_| panic!("enchantable.value must fit an i32"));
                assert!(value > 0, "enchantable.value must be positive");
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::ENCHANTABLE,
                        Some(vanilla_components::Enchantable::from_extracted_value(#value)),
                    )
                });
            }
            "minecraft:damage_resistant" => {
                let types = holder_set_component_field(value, "damage_resistant", "types");
                let types = holder_set_token(types, "damage_resistant", damage_type_ref_token);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::DAMAGE_RESISTANT,
                        Some(vanilla_components::DamageResistant::new(#types)),
                    )
                });
            }
            "minecraft:repairable" => {
                let items = holder_set_component_field(value, "repairable", "items");
                let items = holder_set_token(items, "repairable", |item| {
                    item_ref_token(item, "repairable")
                });
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::REPAIRABLE,
                        Some(vanilla_components::Repairable::new(#items)),
                    )
                });
            }
            "minecraft:dye" => {
                let color = dye_color_token(value);
                builder_calls.push(quote! {
                    .builder_set(vanilla_components::DYE, Some(#color))
                });
            }
            "minecraft:map_color" => {
                let rgb = component_i32(value, "map_color");
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::MAP_COLOR,
                        Some(vanilla_components::MapItemColor::new(#rgb)),
                    )
                });
            }
            "minecraft:ominous_bottle_amplifier" => {
                let amplifier = component_i32(value, "ominous_bottle_amplifier");
                assert!(
                    (0..=4).contains(&amplifier),
                    "ominous_bottle_amplifier must be in 0..=4"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::OMINOUS_BOTTLE_AMPLIFIER,
                        Some(vanilla_components::OminousBottleAmplifier::new(#amplifier)),
                    )
                });
            }
            "minecraft:instrument" => {
                let instrument = instrument_ref_token(value);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::INSTRUMENT,
                        Some(vanilla_components::InstrumentComponent::new(
                            crate::RegistryHolder::reference(#instrument),
                        )),
                    )
                });
            }
            "minecraft:provides_trim_material" => {
                let material = trim_material_ref_token(value);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::PROVIDES_TRIM_MATERIAL,
                        Some(vanilla_components::ProvidesTrimMaterial::new(
                            crate::RegistryHolder::reference(#material),
                        )),
                    )
                });
            }
            "minecraft:jukebox_playable" => {
                let song = jukebox_song_ref_token(value);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::JUKEBOX_PLAYABLE,
                        Some(vanilla_components::JukeboxPlayable::new(
                            #song,
                        )),
                    )
                });
            }
            "minecraft:provides_banner_patterns" => {
                let patterns =
                    holder_set_token(value, "provides_banner_patterns", banner_pattern_ref_token);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::PROVIDES_BANNER_PATTERNS,
                        Some(#patterns),
                    )
                });
            }
            "minecraft:recipes" => {
                assert!(
                    value.as_array().is_some_and(Vec::is_empty),
                    "vanilla item prototypes currently require empty recipes, got {value}"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::RECIPES,
                        Some(vanilla_components::Recipes::empty()),
                    )
                });
            }
            "minecraft:banner_patterns" => {
                assert!(
                    value.as_array().is_some_and(Vec::is_empty),
                    "vanilla item prototypes currently require empty banner patterns, got {value}"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::BANNER_PATTERNS,
                        Some(vanilla_components::BannerPatternLayers::empty()),
                    )
                });
            }
            "minecraft:pot_decorations" => {
                let decorations = value
                    .as_array()
                    .unwrap_or_else(|| panic!("pot_decorations must be an item list, got {value}"));
                assert!(
                    decorations.len() == 4
                        && decorations
                            .iter()
                            .all(|decoration| decoration.as_str() == Some("minecraft:brick")),
                    "extracted decorated pot must use four brick placeholders"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::POT_DECORATIONS,
                        Some(vanilla_components::PotDecorations::EMPTY),
                    )
                });
            }
            "minecraft:use_remainder" => {
                let remainder = value.as_object().unwrap_or_else(|| {
                    panic!("use_remainder must be an item template, got {value}")
                });
                assert_eq!(
                    remainder.len(),
                    1,
                    "extracted use remainders currently require an item-only template"
                );
                let item = remainder
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("use_remainder.id must be an item identifier"));
                let item = item_ref_token(item, "use_remainder");
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::USE_REMAINDER,
                        Some(vanilla_components::UseRemainder::new(
                            vanilla_components::ItemStackTemplate::new(#item),
                        )),
                    )
                });
            }
            "minecraft:charged_projectiles" => {
                assert!(
                    value.as_array().is_some_and(Vec::is_empty),
                    "vanilla item prototypes currently require empty charged projectiles, got {value}"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::CHARGED_PROJECTILES,
                        Some(vanilla_components::ChargedProjectiles::empty()),
                    )
                });
            }
            "minecraft:bundle_contents" => {
                assert!(
                    value.as_array().is_some_and(Vec::is_empty),
                    "vanilla item prototypes currently require empty bundle contents, got {value}"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::BUNDLE_CONTENTS,
                        Some(vanilla_components::BundleContents::empty()),
                    )
                });
            }
            "minecraft:container" => {
                assert!(
                    value.as_array().is_some_and(Vec::is_empty),
                    "vanilla item prototypes currently require empty container contents, got {value}"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::CONTAINER,
                        Some(vanilla_components::ItemContainerContents::empty()),
                    )
                });
            }
            "minecraft:tooltip_style" | "minecraft:note_block_sound" => {
                let identifier = value
                    .as_str()
                    .unwrap_or_else(|| panic!("{key} component must be an identifier string"));
                let identifier = identifier_token(identifier);
                builder_calls.push(quote! {
                    .builder_set(vanilla_components::#component_ident, Some(#identifier))
                });
            }
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
            "minecraft:use_effects" => {
                if let Some(use_effects) = use_effects_component_token(value) {
                    builder_calls.push(quote! {
                        .builder_set(vanilla_components::USE_EFFECTS, Some(#use_effects))
                    });
                }
            }
            "minecraft:lore" => {
                assert!(
                    value.as_array().is_some_and(Vec::is_empty),
                    "vanilla item prototypes currently require empty lore, got {value}"
                );
            }
            "minecraft:enchantments" => {
                assert!(
                    value.as_object().is_some_and(serde_json::Map::is_empty),
                    "vanilla item prototypes currently require default empty enchantments, got {value}"
                );
            }
            "minecraft:stored_enchantments" => {
                assert!(
                    value.as_object().is_some_and(serde_json::Map::is_empty),
                    "vanilla item prototypes currently require empty stored enchantments, got {value}"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::STORED_ENCHANTMENTS,
                        Some(vanilla_components::ItemEnchantments::empty()),
                    )
                });
            }
            "minecraft:rarity" => {
                if let Some(rarity) = rarity_component_token(value) {
                    builder_calls
                        .push(quote! { .builder_set(vanilla_components::RARITY, Some(#rarity)) });
                }
            }
            "minecraft:tooltip_display" => {
                assert!(
                    value.as_object().is_some_and(serde_json::Map::is_empty),
                    "vanilla item prototypes currently require the default tooltip display, got {value}"
                );
            }
            "minecraft:swing_animation" => {
                if let Some(swing_animation) = swing_animation_component_token(value) {
                    builder_calls.push(quote! {
                        .builder_set(
                            vanilla_components::SWING_ANIMATION,
                            Some(#swing_animation),
                        )
                    });
                }
            }
            "minecraft:break_sound" => {
                if value.as_str() != Some("minecraft:entity.item.break") {
                    let break_sound = sound_event_value_token(value, "break_sound");
                    builder_calls.push(quote! {
                        .builder_set(vanilla_components::BREAK_SOUND, Some(#break_sound))
                    });
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
                        _ => panic!("unknown equippable slot {slot_str:?}"),
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
            _ => panic!(
                "unsupported extracted component {key} on item {}",
                item.name
            ),
        }
    }

    builder_calls
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
