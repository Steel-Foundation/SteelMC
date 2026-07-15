use std::fs;

use crate::generator_functions::generate_sound_event_ref;
use heck::{ToShoutySnakeCase, ToSnakeCase};
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use serde::{Deserialize, de};
use steel_utils::Identifier;
use steel_utils::types::GameType;

#[derive(Deserialize, Debug)]
struct EnchantmentJson {
    max_level: u32,
    min_cost: CostJson,
    max_cost: CostJson,
    anvil_cost: i32,
    weight: u32,
    slots: Vec<String>,
    supported_items: String,
    primary_items: Option<String>,
    exclusive_set: Option<String>,
    #[serde(default)]
    effects: EnchantmentEffectsJson,
}

#[derive(Deserialize, Debug)]
struct CostJson {
    base: i32,
    per_level_above_first: i32,
}

#[derive(Deserialize, Debug, Default)]
struct EnchantmentEffectsJson {
    #[serde(rename = "minecraft:damage_protection", default)]
    damage_protection: Vec<ConditionalValueEffectJson>,
    #[serde(rename = "minecraft:damage_immunity", default)]
    damage_immunity: Vec<ConditionalDamageImmunityEffectJson>,
    #[serde(rename = "minecraft:damage", default)]
    damage: Vec<ConditionalValueEffectJson>,
    #[serde(rename = "minecraft:smash_damage_per_fallen_block", default)]
    smash_damage_per_fallen_block: Vec<ConditionalValueEffectJson>,
    #[serde(rename = "minecraft:knockback", default)]
    knockback: Vec<ConditionalValueEffectJson>,
    #[serde(rename = "minecraft:armor_effectiveness", default)]
    armor_effectiveness: Vec<ConditionalValueEffectJson>,
    #[serde(rename = "minecraft:post_attack", default)]
    post_attack: Vec<TargetedConditionalEntityEffectJson>,
    #[serde(rename = "minecraft:post_piercing_attack", default)]
    post_piercing_attack: Vec<ConditionalEntityEffectJson>,
    #[serde(rename = "minecraft:hit_block", default)]
    hit_block: Vec<serde_json::Value>,
    #[serde(rename = "minecraft:item_damage", default)]
    item_damage: Vec<ConditionalValueEffectJson>,
    #[serde(rename = "minecraft:equipment_drops", default)]
    equipment_drops: Vec<TargetedConditionalValueEffectJson>,
    #[serde(rename = "minecraft:location_changed", default)]
    location_changed: Vec<serde_json::Value>,
    #[serde(rename = "minecraft:tick", default)]
    tick: Vec<serde_json::Value>,
    #[serde(rename = "minecraft:ammo_use", default)]
    ammo_use: Vec<ConditionalValueEffectJson>,
    #[serde(rename = "minecraft:projectile_piercing", default)]
    projectile_piercing: Vec<ConditionalValueEffectJson>,
    #[serde(rename = "minecraft:projectile_spawned", default)]
    projectile_spawned: Vec<ConditionalEntityEffectJson>,
    #[serde(rename = "minecraft:projectile_spread", default)]
    projectile_spread: Vec<ConditionalValueEffectJson>,
    #[serde(rename = "minecraft:projectile_count", default)]
    projectile_count: Vec<ConditionalValueEffectJson>,
    #[serde(rename = "minecraft:trident_return_acceleration", default)]
    trident_return_acceleration: Vec<ConditionalValueEffectJson>,
    #[serde(rename = "minecraft:fishing_time_reduction", default)]
    fishing_time_reduction: Vec<ConditionalValueEffectJson>,
    #[serde(rename = "minecraft:fishing_luck_bonus", default)]
    fishing_luck_bonus: Vec<ConditionalValueEffectJson>,
    #[serde(rename = "minecraft:block_experience", default)]
    block_experience: Vec<ConditionalValueEffectJson>,
    #[serde(rename = "minecraft:mob_experience", default)]
    mob_experience: Vec<ConditionalValueEffectJson>,
    #[serde(rename = "minecraft:repair_with_xp", default)]
    repair_with_xp: Vec<ConditionalValueEffectJson>,
    #[serde(rename = "minecraft:attributes", default)]
    attributes: Vec<AttributeEffectJson>,
    #[serde(rename = "minecraft:crossbow_charge_time", default)]
    crossbow_charge_time: Option<ValueEffectJson>,
    #[serde(rename = "minecraft:crossbow_charging_sounds", default)]
    crossbow_charging_sounds: Vec<CrossbowChargingSoundsJson>,
    #[serde(rename = "minecraft:trident_sound", default)]
    trident_sound: Vec<Identifier>,
    #[serde(rename = "minecraft:prevent_equipment_drop", default)]
    prevent_equipment_drop: Option<serde_json::Value>,
    #[serde(rename = "minecraft:prevent_armor_change", default)]
    prevent_armor_change: Option<serde_json::Value>,
    #[serde(rename = "minecraft:trident_spin_attack_strength", default)]
    trident_spin_attack_strength: Option<ValueEffectJson>,
}

#[derive(Deserialize, Debug)]
struct ConditionalValueEffectJson {
    effect: ValueEffectJson,
    #[serde(default)]
    requirements: Option<RequirementsJson>,
}

#[derive(Deserialize, Debug)]
struct ConditionalEntityEffectJson {
    effect: EntityEffectJson,
    #[serde(default)]
    requirements: Option<RequirementsJson>,
}

#[derive(Debug)]
struct ConditionalDamageImmunityEffectJson {
    requirements: Option<RequirementsJson>,
}

impl<'de> Deserialize<'de> for ConditionalDamageImmunityEffectJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let Some(object) = value.as_object() else {
            return Err(de::Error::custom(
                "damage_immunity effect entry must be an object",
            ));
        };

        for key in object.keys() {
            if key != "effect" && key != "requirements" {
                return Err(de::Error::custom(format!(
                    "unsupported damage_immunity field `{key}`"
                )));
            }
        }

        let Some(effect) = object.get("effect") else {
            return Err(de::Error::custom(
                "damage_immunity effect entry missing `effect`",
            ));
        };
        let Some(effect_object) = effect.as_object() else {
            return Err(de::Error::custom(
                "damage_immunity `effect` must be an object",
            ));
        };
        if !effect_object.is_empty() {
            return Err(de::Error::custom(
                "damage_immunity `effect` must be an empty object",
            ));
        }

        let requirements = object
            .get("requirements")
            .map(parse_requirements_json)
            .transpose()
            .map_err(de::Error::custom)?;

        Ok(Self { requirements })
    }
}

#[derive(Deserialize, Debug)]
struct TargetedConditionalEntityEffectJson {
    effect: EntityEffectJson,
    enchanted: EnchantmentTargetJson,
    affected: EnchantmentTargetJson,
    #[serde(default)]
    requirements: Option<RequirementsJson>,
}

#[derive(Deserialize, Debug)]
struct TargetedConditionalValueEffectJson {
    effect: ValueEffectJson,
    enchanted: EnchantmentTargetJson,
    #[serde(default)]
    requirements: Option<RequirementsJson>,
}

#[derive(Debug)]
enum EnchantmentTargetJson {
    Attacker,
    DamagingEntity,
    Victim,
}

impl<'de> Deserialize<'de> for EnchantmentTargetJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        parse_enchantment_target(&raw).map_err(de::Error::custom)
    }
}

#[derive(Debug)]
enum EntityEffectJson {
    AllOf(Vec<EntityEffectJson>),
    ChangeItemDamage {
        amount: LevelBasedValueJson,
    },
    ApplyExhaustion {
        amount: LevelBasedValueJson,
    },
    ApplyImpulse {
        direction: [f64; 3],
        coordinate_scale: [f64; 3],
        magnitude: LevelBasedValueJson,
    },
    PlaySound {
        sounds: Vec<Identifier>,
        volume: f32,
        pitch: f32,
    },
    DamageEntity {
        min_damage: LevelBasedValueJson,
        max_damage: LevelBasedValueJson,
        damage_type: Identifier,
    },
    Ignite {
        duration: LevelBasedValueJson,
    },
    ApplyMobEffect {
        to_apply: MobEffectSelectionJson,
        min_duration: LevelBasedValueJson,
        max_duration: LevelBasedValueJson,
        min_amplifier: LevelBasedValueJson,
        max_amplifier: LevelBasedValueJson,
    },
    Unsupported {
        effect_type: Identifier,
    },
}

impl<'de> Deserialize<'de> for EntityEffectJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        parse_entity_effect_json(&value).map_err(de::Error::custom)
    }
}

#[derive(Debug)]
enum MobEffectSelectionJson {
    Single(Identifier),
    UnsupportedTag(Identifier),
}

#[derive(Debug)]
enum RequirementsJson {
    AllOf(Vec<RequirementsJson>),
    AnyOf(Vec<RequirementsJson>),
    Inverted(Box<RequirementsJson>),
    EntityProperties {
        entity: EntityTargetJson,
        predicate: EntityPredicateJson,
    },
    DamageSourceProperties(DamageSourcePredicateJson),
    RandomChance {
        chance: LevelBasedValueJson,
    },
    MatchTool {
        items: Option<ItemHolderSetJson>,
    },
    Unsupported {
        condition: Identifier,
    },
}

#[derive(Debug)]
enum ItemHolderSetJson {
    Tag(Identifier),
    Direct(Vec<Identifier>),
}

impl<'de> Deserialize<'de> for RequirementsJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        parse_requirements_json(&value).map_err(de::Error::custom)
    }
}

#[derive(Debug)]
enum EntityTargetJson {
    This,
    Attacker,
    DirectAttacker,
}

#[derive(Debug)]
struct EntityPredicateJson {
    entity_type: EntityTypePredicateJson,
    vehicle: EntityVehiclePredicateJson,
    flags: EntityFlagsPredicateJson,
    type_specific: EntityTypeSpecificPredicateJson,
    unsupported: bool,
}

#[derive(Debug)]
enum EntityTypePredicateJson {
    Any,
    Type(Identifier),
    Tag(Identifier),
}

#[derive(Debug)]
enum EntityVehiclePredicateJson {
    Any,
    Present,
    Unsupported,
}

#[derive(Debug)]
struct EntityFlagsPredicateJson {
    is_fall_flying: Option<bool>,
    is_in_water: Option<bool>,
    unsupported: bool,
}

impl EntityFlagsPredicateJson {
    const fn any() -> Self {
        Self {
            is_fall_flying: None,
            is_in_water: None,
            unsupported: false,
        }
    }
}

#[derive(Debug)]
enum EntityTypeSpecificPredicateJson {
    Any,
    Player(PlayerPredicateJson),
    Unsupported,
}

#[derive(Debug)]
struct PlayerPredicateJson {
    game_modes: Vec<GameType>,
    food_level_min: Option<i32>,
    unsupported: bool,
}

#[derive(Debug)]
struct DamageSourcePredicateJson {
    tags: Vec<DamageSourceTagPredicateJson>,
    is_direct: Option<bool>,
}

#[derive(Debug)]
struct DamageSourceTagPredicateJson {
    tag: Identifier,
    expected: bool,
}

#[derive(Deserialize, Debug)]
struct AttributeEffectJson {
    amount: LevelBasedValueJson,
    attribute: Identifier,
    id: Identifier,
    operation: String,
}

#[derive(Deserialize, Debug)]
struct CrossbowChargingSoundsJson {
    start: Option<Identifier>,
    mid: Option<Identifier>,
    end: Option<Identifier>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
enum ValueEffectJson {
    #[serde(rename = "minecraft:add")]
    Add { value: LevelBasedValueJson },
    #[serde(rename = "minecraft:set")]
    Set { value: LevelBasedValueJson },
    #[serde(rename = "minecraft:multiply")]
    Multiply { factor: LevelBasedValueJson },
    #[serde(rename = "minecraft:remove_binomial")]
    RemoveBinomial { chance: LevelBasedValueJson },
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum LevelBasedValueJson {
    Constant(f32),
    Typed(LevelBasedValueTypedJson),
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
enum LevelBasedValueTypedJson {
    #[serde(rename = "minecraft:clamped")]
    Clamped {
        value: Box<LevelBasedValueJson>,
        min: f32,
        max: f32,
    },
    #[serde(rename = "minecraft:exponent")]
    Exponent {
        base: Box<LevelBasedValueJson>,
        power: Box<LevelBasedValueJson>,
    },
    #[serde(rename = "minecraft:fraction")]
    Fraction {
        numerator: Box<LevelBasedValueJson>,
        denominator: Box<LevelBasedValueJson>,
    },
    #[serde(rename = "minecraft:levels_squared")]
    LevelsSquared { added: f32 },
    #[serde(rename = "minecraft:linear")]
    Linear {
        base: f32,
        per_level_above_first: f32,
    },
    #[serde(rename = "minecraft:lookup")]
    Lookup {
        values: Vec<f32>,
        fallback: Box<LevelBasedValueJson>,
    },
}

fn slot_to_tokens(slot: &str) -> TokenStream {
    match slot {
        "any" => quote! { EquipmentSlotGroup::Any },
        "hand" => quote! { EquipmentSlotGroup::Hand },
        "mainhand" => quote! { EquipmentSlotGroup::MainHand },
        "offhand" => quote! { EquipmentSlotGroup::OffHand },
        "armor" => quote! { EquipmentSlotGroup::Armor },
        "head" => quote! { EquipmentSlotGroup::Head },
        "chest" => quote! { EquipmentSlotGroup::Chest },
        "legs" => quote! { EquipmentSlotGroup::Legs },
        "feet" => quote! { EquipmentSlotGroup::Feet },
        "body" => quote! { EquipmentSlotGroup::Body },
        other => panic!("Unknown equipment slot group: {other}"),
    }
}

fn identifier_token(identifier: &Identifier) -> TokenStream {
    let namespace = identifier.namespace.as_ref();
    let path = identifier.path.as_ref();
    quote! { Identifier::new_static(#namespace, #path) }
}

fn damage_type_ref_token(identifier: &Identifier) -> TokenStream {
    assert_eq!(
        identifier.namespace.as_ref(),
        "minecraft",
        "vanilla enchantment damage_type references must use the minecraft namespace: {identifier}"
    );
    let ident = Ident::new(&identifier.path.to_ascii_uppercase(), Span::call_site());
    quote! { &crate::vanilla_damage_types::#ident }
}

fn parse_identifier(raw: &str) -> Result<Identifier, String> {
    raw.parse::<Identifier>()
        .map_err(|error| format!("invalid identifier {raw}: {error}"))
}

fn object_field<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<&'a serde_json::Value, String> {
    object
        .get(field)
        .ok_or_else(|| format!("missing enchantment requirement field `{field}`"))
}

fn string_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<String, String> {
    object_field(object, field)?
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("enchantment requirement field `{field}` must be a string"))
}

fn parse_entity_target(raw: &str) -> Result<EntityTargetJson, String> {
    match raw {
        "this" => Ok(EntityTargetJson::This),
        "attacker" => Ok(EntityTargetJson::Attacker),
        "direct_attacker" => Ok(EntityTargetJson::DirectAttacker),
        other => Err(format!("unsupported enchantment entity target `{other}`")),
    }
}

fn parse_enchantment_target(raw: &str) -> Result<EnchantmentTargetJson, String> {
    match raw {
        "attacker" => Ok(EnchantmentTargetJson::Attacker),
        "damaging_entity" => Ok(EnchantmentTargetJson::DamagingEntity),
        "victim" => Ok(EnchantmentTargetJson::Victim),
        other => Err(format!(
            "unsupported enchantment post-attack target `{other}`"
        )),
    }
}

fn parse_level_based_value_json(value: &serde_json::Value) -> Result<LevelBasedValueJson, String> {
    serde_json::from_value(value.to_owned())
        .map_err(|error| format!("invalid level-based value: {error}"))
}

fn parse_random_chance_value(value: &serde_json::Value) -> Result<LevelBasedValueJson, String> {
    let Some(object) = value.as_object() else {
        return parse_level_based_value_json(value);
    };
    if object.get("type").and_then(serde_json::Value::as_str) != Some("minecraft:enchantment_level")
    {
        return parse_level_based_value_json(value);
    }
    parse_level_based_value_json(object_field(object, "amount")?)
}

fn parse_mob_effect_selection_json(
    value: &serde_json::Value,
) -> Result<MobEffectSelectionJson, String> {
    let raw = value
        .as_str()
        .ok_or_else(|| "mob effect selection must be a string".to_owned())?;
    let Some(tag) = raw.strip_prefix('#') else {
        return Ok(MobEffectSelectionJson::Single(parse_identifier(raw)?));
    };

    Ok(MobEffectSelectionJson::UnsupportedTag(parse_identifier(
        tag,
    )?))
}

fn parse_entity_effect_json(value: &serde_json::Value) -> Result<EntityEffectJson, String> {
    let Some(object) = value.as_object() else {
        return Err("enchantment entity effect must be an object".to_owned());
    };
    let effect_type = string_field(object, "type")?;

    match effect_type.as_str() {
        "minecraft:all_of" => {
            let effects = object_field(object, "effects")?
                .as_array()
                .ok_or_else(|| "all_of entity effect `effects` must be an array".to_owned())?
                .iter()
                .map(parse_entity_effect_json)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(EntityEffectJson::AllOf(effects))
        }
        "minecraft:change_item_damage" => {
            for key in object.keys() {
                if key != "type" && key != "amount" {
                    return Err(format!(
                        "unsupported change_item_damage effect field `{key}`"
                    ));
                }
            }
            Ok(EntityEffectJson::ChangeItemDamage {
                amount: parse_level_based_value_json(object_field(object, "amount")?)?,
            })
        }
        "minecraft:apply_exhaustion" => {
            for key in object.keys() {
                if key != "type" && key != "amount" {
                    return Err(format!("unsupported apply_exhaustion effect field `{key}`"));
                }
            }
            Ok(EntityEffectJson::ApplyExhaustion {
                amount: parse_level_based_value_json(object_field(object, "amount")?)?,
            })
        }
        "minecraft:apply_impulse" => {
            for key in object.keys() {
                if !matches!(
                    key.as_str(),
                    "type" | "direction" | "coordinate_scale" | "magnitude"
                ) {
                    return Err(format!("unsupported apply_impulse effect field `{key}`"));
                }
            }
            Ok(EntityEffectJson::ApplyImpulse {
                direction: parse_vec3_json(object_field(object, "direction")?)?,
                coordinate_scale: parse_vec3_json(object_field(object, "coordinate_scale")?)?,
                magnitude: parse_level_based_value_json(object_field(object, "magnitude")?)?,
            })
        }
        "minecraft:damage_entity" => {
            for key in object.keys() {
                if !matches!(
                    key.as_str(),
                    "type" | "min_damage" | "max_damage" | "damage_type"
                ) {
                    return Err(format!("unsupported damage_entity effect field `{key}`"));
                }
            }
            Ok(EntityEffectJson::DamageEntity {
                min_damage: parse_level_based_value_json(object_field(object, "min_damage")?)?,
                max_damage: parse_level_based_value_json(object_field(object, "max_damage")?)?,
                damage_type: parse_identifier(&string_field(object, "damage_type")?)?,
            })
        }
        "minecraft:play_sound" => {
            for key in object.keys() {
                if !matches!(key.as_str(), "type" | "sound" | "volume" | "pitch") {
                    return Err(format!("unsupported play_sound effect field `{key}`"));
                }
            }
            Ok(EntityEffectJson::PlaySound {
                sounds: parse_sound_list_json(object_field(object, "sound")?)?,
                volume: parse_f32_field(object, "volume")?,
                pitch: parse_f32_field(object, "pitch")?,
            })
        }
        "minecraft:ignite" => {
            for key in object.keys() {
                if key != "type" && key != "duration" {
                    return Err(format!("unsupported ignite effect field `{key}`"));
                }
            }
            Ok(EntityEffectJson::Ignite {
                duration: parse_level_based_value_json(object_field(object, "duration")?)?,
            })
        }
        "minecraft:apply_mob_effect" => {
            for key in object.keys() {
                if !matches!(
                    key.as_str(),
                    "type"
                        | "to_apply"
                        | "min_duration"
                        | "max_duration"
                        | "min_amplifier"
                        | "max_amplifier"
                ) {
                    return Err(format!("unsupported apply_mob_effect field `{key}`"));
                }
            }
            Ok(EntityEffectJson::ApplyMobEffect {
                to_apply: parse_mob_effect_selection_json(object_field(object, "to_apply")?)?,
                min_duration: parse_level_based_value_json(object_field(object, "min_duration")?)?,
                max_duration: parse_level_based_value_json(object_field(object, "max_duration")?)?,
                min_amplifier: parse_level_based_value_json(object_field(
                    object,
                    "min_amplifier",
                )?)?,
                max_amplifier: parse_level_based_value_json(object_field(
                    object,
                    "max_amplifier",
                )?)?,
            })
        }
        _ => Ok(EntityEffectJson::Unsupported {
            effect_type: parse_identifier(&effect_type)?,
        }),
    }
}

fn parse_vec3_json(value: &serde_json::Value) -> Result<[f64; 3], String> {
    let Some(values) = value.as_array() else {
        return Err("vec3 must be an array".to_owned());
    };
    let [x, y, z] = values.as_slice() else {
        return Err("vec3 must have exactly three values".to_owned());
    };

    Ok([
        x.as_f64()
            .ok_or_else(|| "vec3 x must be a number".to_owned())?,
        y.as_f64()
            .ok_or_else(|| "vec3 y must be a number".to_owned())?,
        z.as_f64()
            .ok_or_else(|| "vec3 z must be a number".to_owned())?,
    ])
}

fn parse_sound_list_json(value: &serde_json::Value) -> Result<Vec<Identifier>, String> {
    match value {
        serde_json::Value::String(raw) => Ok(vec![parse_identifier(raw)?]),
        serde_json::Value::Array(values) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| "sound list entries must be strings".to_owned())
                    .and_then(parse_identifier)
            })
            .collect(),
        _ => Err("play_sound `sound` must be a string or string array".to_owned()),
    }
}

fn parse_f32_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<f32, String> {
    let value = object_field(object, field)?;
    let number = value
        .as_f64()
        .ok_or_else(|| format!("`{field}` must be a number"))?;
    Ok(number as f32)
}

fn parse_entity_type_predicate(raw: &str) -> Result<EntityTypePredicateJson, String> {
    let Some(tag) = raw.strip_prefix('#') else {
        return Ok(EntityTypePredicateJson::Type(parse_identifier(raw)?));
    };

    Ok(EntityTypePredicateJson::Tag(parse_identifier(tag)?))
}

fn parse_entity_predicate_json(value: &serde_json::Value) -> Result<EntityPredicateJson, String> {
    let Some(object) = value.as_object() else {
        return Err("entity_properties predicate must be an object".to_owned());
    };
    let unsupported = object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "type"
                | "minecraft:entity_type"
                | "vehicle"
                | "minecraft:vehicle"
                | "flags"
                | "minecraft:flags"
                | "type_specific"
                | "minecraft:type_specific/player"
        )
    });
    let entity_type = match aliased_object_field(object, &["type", "minecraft:entity_type"])? {
        Some(serde_json::Value::String(raw)) => parse_entity_type_predicate(raw)?,
        Some(_) => return Err("entity_properties predicate `type` must be a string".to_owned()),
        None => EntityTypePredicateJson::Any,
    };
    let vehicle = match aliased_object_field(object, &["vehicle", "minecraft:vehicle"])? {
        Some(serde_json::Value::Object(vehicle)) if vehicle.is_empty() => {
            EntityVehiclePredicateJson::Present
        }
        Some(serde_json::Value::Object(_)) => EntityVehiclePredicateJson::Unsupported,
        Some(_) => return Err("entity_properties predicate `vehicle` must be an object".to_owned()),
        None => EntityVehiclePredicateJson::Any,
    };
    let flags = aliased_object_field(object, &["flags", "minecraft:flags"])?
        .map(parse_entity_flags_predicate_json)
        .transpose()?
        .unwrap_or_else(EntityFlagsPredicateJson::any);
    let type_specific =
        match aliased_object_field(object, &["type_specific", "minecraft:type_specific/player"])? {
            Some(value) if object.contains_key("minecraft:type_specific/player") => {
                EntityTypeSpecificPredicateJson::Player(parse_player_predicate_json(value, false)?)
            }
            Some(value) => parse_type_specific_predicate_json(value)?,
            None => EntityTypeSpecificPredicateJson::Any,
        };

    Ok(EntityPredicateJson {
        entity_type,
        vehicle,
        flags,
        type_specific,
        unsupported,
    })
}

fn aliased_object_field<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    fields: &[&str],
) -> Result<Option<&'a serde_json::Value>, String> {
    let mut found: Option<(&str, &serde_json::Value)> = None;
    for field in fields {
        if let Some(value) = object.get(*field) {
            if let Some(previous) = found {
                let previous_field = previous.0;
                return Err(format!(
                    "entity_properties predicate must not contain both `{previous_field}` and `{field}`"
                ));
            }
            found = Some((*field, value));
        }
    }

    Ok(found.map(|(_, value)| value))
}

fn parse_entity_flags_predicate_json(
    value: &serde_json::Value,
) -> Result<EntityFlagsPredicateJson, String> {
    let Some(object) = value.as_object() else {
        return Err("entity flags predicate must be an object".to_owned());
    };
    let unsupported = object
        .keys()
        .any(|key| key != "is_fall_flying" && key != "is_in_water");
    let is_fall_flying = optional_bool_field(object, "is_fall_flying")?;
    let is_in_water = optional_bool_field(object, "is_in_water")?;

    Ok(EntityFlagsPredicateJson {
        is_fall_flying,
        is_in_water,
        unsupported,
    })
}

fn parse_type_specific_predicate_json(
    value: &serde_json::Value,
) -> Result<EntityTypeSpecificPredicateJson, String> {
    let Some(object) = value.as_object() else {
        return Err("entity type_specific predicate must be an object".to_owned());
    };
    let predicate_type = string_field(object, "type")?;
    if predicate_type != "minecraft:player" {
        return Ok(EntityTypeSpecificPredicateJson::Unsupported);
    }

    Ok(EntityTypeSpecificPredicateJson::Player(
        parse_player_predicate_json(value, true)?,
    ))
}

fn parse_player_predicate_json(
    value: &serde_json::Value,
    allow_type_field: bool,
) -> Result<PlayerPredicateJson, String> {
    let Some(object) = value.as_object() else {
        return Err("player predicate must be an object".to_owned());
    };
    let unsupported = object.keys().any(|key| {
        !matches!(key.as_str(), "gamemode" | "food") && !(allow_type_field && key == "type")
    });
    let game_modes = match object.get("gamemode") {
        Some(serde_json::Value::Array(modes)) => modes
            .iter()
            .map(|mode| {
                mode.as_str()
                    .ok_or_else(|| "player gamemode entries must be strings".to_owned())
                    .and_then(parse_game_type)
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err("player predicate `gamemode` must be an array".to_owned()),
        None => Vec::new(),
    };
    let food_level_min = object
        .get("food")
        .map(parse_player_food_min_json)
        .transpose()?;

    Ok(PlayerPredicateJson {
        game_modes,
        food_level_min,
        unsupported,
    })
}

fn parse_player_food_min_json(value: &serde_json::Value) -> Result<i32, String> {
    let Some(object) = value.as_object() else {
        return Err("player food predicate must be an object".to_owned());
    };
    let level = object_field(object, "level")?;
    let Some(level_object) = level.as_object() else {
        return Err("player food `level` must be an object".to_owned());
    };
    for key in object.keys() {
        if key != "level" {
            return Err(format!("unsupported player food predicate field `{key}`"));
        }
    }
    for key in level_object.keys() {
        if key != "min" {
            return Err(format!("unsupported player food level field `{key}`"));
        }
    }
    let min = object_field(level_object, "min")?
        .as_i64()
        .ok_or_else(|| "player food level `min` must be an integer".to_owned())?;
    i32::try_from(min).map_err(|_| "player food level `min` out of range".to_owned())
}

fn optional_bool_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<Option<bool>, String> {
    match object.get(field) {
        Some(serde_json::Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(format!("`{field}` must be a bool")),
        None => Ok(None),
    }
}

fn parse_game_type(value: &str) -> Result<GameType, String> {
    match value {
        "survival" => Ok(GameType::Survival),
        "creative" => Ok(GameType::Creative),
        "adventure" => Ok(GameType::Adventure),
        "spectator" => Ok(GameType::Spectator),
        other => Err(format!("unknown game type `{other}`")),
    }
}

fn parse_damage_source_predicate_json(
    value: &serde_json::Value,
) -> Result<DamageSourcePredicateJson, String> {
    let Some(object) = value.as_object() else {
        return Err("damage_source_properties predicate must be an object".to_owned());
    };
    for key in object.keys() {
        if key != "tags" && key != "is_direct" {
            return Err(format!(
                "unsupported damage_source_properties predicate field `{key}`"
            ));
        }
    }
    let tags = match object.get("tags") {
        Some(serde_json::Value::Array(tags)) => tags
            .iter()
            .map(parse_damage_source_tag_predicate_json)
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => {
            return Err("damage_source_properties predicate `tags` must be an array".to_owned());
        }
        None => Vec::new(),
    };
    let is_direct = match object.get("is_direct") {
        Some(serde_json::Value::Bool(is_direct)) => Some(*is_direct),
        Some(_) => {
            return Err("damage_source_properties predicate `is_direct` must be a bool".to_owned());
        }
        None => None,
    };

    Ok(DamageSourcePredicateJson { tags, is_direct })
}

fn parse_damage_source_tag_predicate_json(
    value: &serde_json::Value,
) -> Result<DamageSourceTagPredicateJson, String> {
    let Some(object) = value.as_object() else {
        return Err("damage source tag predicate must be an object".to_owned());
    };
    let id = string_field(object, "id")?;
    let expected = object_field(object, "expected")?
        .as_bool()
        .ok_or_else(|| "damage source tag predicate `expected` must be a bool".to_owned())?;
    for key in object.keys() {
        if key != "id" && key != "expected" {
            return Err(format!("unsupported damage source tag field `{key}`"));
        }
    }

    Ok(DamageSourceTagPredicateJson {
        tag: parse_identifier(&id)?,
        expected,
    })
}

fn parse_requirements_json(value: &serde_json::Value) -> Result<RequirementsJson, String> {
    let Some(object) = value.as_object() else {
        return Err("enchantment effect requirements must be an object".to_owned());
    };
    let condition = string_field(object, "condition")?;

    match condition.as_str() {
        "minecraft:all_of" => {
            let terms = object_field(object, "terms")?
                .as_array()
                .ok_or_else(|| "all_of requirements `terms` must be an array".to_owned())?
                .iter()
                .map(parse_requirements_json)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(RequirementsJson::AllOf(terms))
        }
        "minecraft:any_of" => {
            let terms = object_field(object, "terms")?
                .as_array()
                .ok_or_else(|| "any_of requirements `terms` must be an array".to_owned())?
                .iter()
                .map(parse_requirements_json)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(RequirementsJson::AnyOf(terms))
        }
        "minecraft:inverted" => {
            let term = parse_requirements_json(object_field(object, "term")?)?;
            Ok(RequirementsJson::Inverted(Box::new(term)))
        }
        "minecraft:entity_properties" => {
            let entity = parse_entity_target(&string_field(object, "entity")?)?;
            let predicate = parse_entity_predicate_json(object_field(object, "predicate")?)?;
            Ok(RequirementsJson::EntityProperties { entity, predicate })
        }
        "minecraft:damage_source_properties" => {
            let predicate = parse_damage_source_predicate_json(object_field(object, "predicate")?)?;
            Ok(RequirementsJson::DamageSourceProperties(predicate))
        }
        "minecraft:random_chance" => {
            for key in object.keys() {
                if key != "condition" && key != "chance" {
                    return Err(format!(
                        "unsupported random_chance requirement field `{key}`"
                    ));
                }
            }
            Ok(RequirementsJson::RandomChance {
                chance: parse_random_chance_value(object_field(object, "chance")?)?,
            })
        }
        "minecraft:match_tool" => {
            for key in object.keys() {
                if key != "condition" && key != "predicate" {
                    return Err(format!("unsupported match_tool requirement field `{key}`"));
                }
            }
            let predicate = object_field(object, "predicate")?
                .as_object()
                .ok_or_else(|| "match_tool `predicate` must be an object".to_owned())?;
            for key in predicate.keys() {
                if key != "items" {
                    return Err(format!("unsupported match_tool predicate field `{key}`"));
                }
            }
            let items = predicate
                .get("items")
                .map(parse_item_holder_set_json)
                .transpose()?;
            Ok(RequirementsJson::MatchTool { items })
        }
        _ => Ok(RequirementsJson::Unsupported {
            condition: parse_identifier(&condition)?,
        }),
    }
}

fn parse_item_holder_set_json(value: &serde_json::Value) -> Result<ItemHolderSetJson, String> {
    if let Some(value) = value.as_str() {
        if let Some(tag) = value.strip_prefix('#') {
            return Ok(ItemHolderSetJson::Tag(parse_identifier(tag)?));
        }
        return Ok(ItemHolderSetJson::Direct(vec![parse_identifier(value)?]));
    }

    let values = value
        .as_array()
        .ok_or_else(|| "match_tool `items` must be an item, item tag, or item list".to_owned())?;
    let items = values
        .iter()
        .map(|value| {
            let value = value
                .as_str()
                .ok_or_else(|| "match_tool item list entries must be strings".to_owned())?;
            if value.starts_with('#') {
                return Err("match_tool direct item lists cannot contain tags".to_owned());
            }
            parse_identifier(value)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ItemHolderSetJson::Direct(items))
}

fn attribute_ref_token(attribute: &Identifier) -> TokenStream {
    assert_eq!(
        attribute.namespace.as_ref(),
        "minecraft",
        "vanilla enchantment attribute references must use the minecraft namespace: {attribute}"
    );
    let ident = Ident::new(&attribute.path.to_shouty_snake_case(), Span::call_site());
    quote! { vanilla_attributes::#ident }
}

fn mob_effect_ref_token(effect: &Identifier) -> TokenStream {
    assert_eq!(
        effect.namespace.as_ref(),
        "minecraft",
        "vanilla enchantment mob effect references must use the minecraft namespace: {effect}"
    );
    let ident = Ident::new(&effect.path.to_shouty_snake_case(), Span::call_site());
    quote! { vanilla_mob_effects::#ident }
}

fn attribute_modifier_operation_token(operation: &str) -> TokenStream {
    match operation {
        "add_value" => quote! { AttributeModifierOperation::AddValue },
        "add_multiplied_base" => quote! { AttributeModifierOperation::AddMultipliedBase },
        "add_multiplied_total" => quote! { AttributeModifierOperation::AddMultipliedTotal },
        other => panic!("Unknown enchantment attribute modifier operation: {other}"),
    }
}

fn option_sound_event_ref_token(sound: Option<&Identifier>) -> TokenStream {
    if let Some(sound) = sound {
        let sound = generate_sound_event_ref(sound);
        quote! { Some(#sound) }
    } else {
        quote! { None }
    }
}

fn generate_level_based_value_ref(
    prefix: &str,
    value: &LevelBasedValueJson,
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    let ident = Ident::new(
        &format!("{prefix}_LEVEL_VALUE_{}", *counter),
        Span::call_site(),
    );
    *counter += 1;
    let value = generate_level_based_value(prefix, value, statics, counter);

    statics.extend(quote! {
        static #ident: LevelBasedValue = #value;
    });

    quote! { &#ident }
}

fn generate_level_based_value(
    prefix: &str,
    value: &LevelBasedValueJson,
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    match value {
        LevelBasedValueJson::Constant(value) => quote! { LevelBasedValue::Constant(#value) },
        LevelBasedValueJson::Typed(value) => match value {
            LevelBasedValueTypedJson::Clamped { value, min, max } => {
                let value = generate_level_based_value_ref(prefix, value, statics, counter);
                quote! { LevelBasedValue::Clamped { value: #value, min: #min, max: #max } }
            }
            LevelBasedValueTypedJson::Exponent { base, power } => {
                let base = generate_level_based_value_ref(prefix, base, statics, counter);
                let power = generate_level_based_value_ref(prefix, power, statics, counter);
                quote! { LevelBasedValue::Exponent { base: #base, power: #power } }
            }
            LevelBasedValueTypedJson::Fraction {
                numerator,
                denominator,
            } => {
                let numerator = generate_level_based_value_ref(prefix, numerator, statics, counter);
                let denominator =
                    generate_level_based_value_ref(prefix, denominator, statics, counter);
                quote! { LevelBasedValue::Fraction { numerator: #numerator, denominator: #denominator } }
            }
            LevelBasedValueTypedJson::LevelsSquared { added } => {
                quote! { LevelBasedValue::LevelsSquared { added: #added } }
            }
            LevelBasedValueTypedJson::Linear {
                base,
                per_level_above_first,
            } => {
                quote! { LevelBasedValue::Linear { base: #base, per_level_above_first: #per_level_above_first } }
            }
            LevelBasedValueTypedJson::Lookup { values, fallback } => {
                let fallback = generate_level_based_value_ref(prefix, fallback, statics, counter);
                quote! { LevelBasedValue::Lookup { values: &[#(#values),*], fallback: #fallback } }
            }
        },
    }
}

fn generate_value_effect(
    prefix: &str,
    effect: &ValueEffectJson,
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    match effect {
        ValueEffectJson::Add { value } => {
            let value = generate_level_based_value_ref(prefix, value, statics, counter);
            quote! { EnchantmentValueEffect::Add { value: #value } }
        }
        ValueEffectJson::Set { value } => {
            let value = generate_level_based_value_ref(prefix, value, statics, counter);
            quote! { EnchantmentValueEffect::Set { value: #value } }
        }
        ValueEffectJson::Multiply { factor } => {
            let factor = generate_level_based_value_ref(prefix, factor, statics, counter);
            quote! { EnchantmentValueEffect::Multiply { factor: #factor } }
        }
        ValueEffectJson::RemoveBinomial { chance } => {
            let chance = generate_level_based_value_ref(prefix, chance, statics, counter);
            quote! { EnchantmentValueEffect::RemoveBinomial { chance: #chance } }
        }
    }
}

fn entity_target_token(entity: &EntityTargetJson) -> TokenStream {
    match entity {
        EntityTargetJson::This => quote! { EnchantmentEntityTarget::This },
        EntityTargetJson::Attacker => quote! { EnchantmentEntityTarget::Attacker },
        EntityTargetJson::DirectAttacker => quote! { EnchantmentEntityTarget::DirectAttacker },
    }
}

fn entity_type_predicate_token(predicate: &EntityTypePredicateJson) -> TokenStream {
    match predicate {
        EntityTypePredicateJson::Any => quote! { EntityTypePredicate::Any },
        EntityTypePredicateJson::Type(entity_type) => {
            let entity_type = identifier_token(entity_type);
            quote! { EntityTypePredicate::Type(#entity_type) }
        }
        EntityTypePredicateJson::Tag(tag) => {
            let tag = identifier_token(tag);
            quote! { EntityTypePredicate::Tag(#tag) }
        }
    }
}

fn game_type_token(game_type: GameType) -> TokenStream {
    match game_type {
        GameType::Survival => quote! { GameType::Survival },
        GameType::Creative => quote! { GameType::Creative },
        GameType::Adventure => quote! { GameType::Adventure },
        GameType::Spectator => quote! { GameType::Spectator },
    }
}

fn entity_vehicle_predicate_token(predicate: &EntityVehiclePredicateJson) -> TokenStream {
    match predicate {
        EntityVehiclePredicateJson::Any => quote! { EntityVehiclePredicate::Any },
        EntityVehiclePredicateJson::Present => quote! { EntityVehiclePredicate::Present },
        EntityVehiclePredicateJson::Unsupported => quote! { EntityVehiclePredicate::Unsupported },
    }
}

fn entity_flags_predicate_token(predicate: &EntityFlagsPredicateJson) -> TokenStream {
    let is_fall_flying = if let Some(value) = predicate.is_fall_flying {
        quote! { Some(#value) }
    } else {
        quote! { None }
    };
    let is_in_water = if let Some(value) = predicate.is_in_water {
        quote! { Some(#value) }
    } else {
        quote! { None }
    };
    let unsupported = predicate.unsupported;

    quote! {
        EntityFlagsPredicate {
            is_fall_flying: #is_fall_flying,
            is_in_water: #is_in_water,
            unsupported: #unsupported,
        }
    }
}

fn entity_type_specific_predicate_token(
    predicate: &EntityTypeSpecificPredicateJson,
) -> TokenStream {
    match predicate {
        EntityTypeSpecificPredicateJson::Any => quote! { EntityTypeSpecificPredicate::Any },
        EntityTypeSpecificPredicateJson::Player(player) => {
            let game_modes = player.game_modes.iter().copied().map(game_type_token);
            let food_level_min = if let Some(min) = player.food_level_min {
                quote! { Some(#min) }
            } else {
                quote! { None }
            };
            let unsupported = player.unsupported;
            quote! {
                EntityTypeSpecificPredicate::Player(PlayerPredicate {
                    game_modes: &[#(#game_modes),*],
                    food_level_min: #food_level_min,
                    unsupported: #unsupported,
                })
            }
        }
        EntityTypeSpecificPredicateJson::Unsupported => {
            quote! { EntityTypeSpecificPredicate::Unsupported }
        }
    }
}

fn entity_predicate_token(predicate: &EntityPredicateJson) -> TokenStream {
    let entity_type = entity_type_predicate_token(&predicate.entity_type);
    let vehicle = entity_vehicle_predicate_token(&predicate.vehicle);
    let flags = entity_flags_predicate_token(&predicate.flags);
    let type_specific = entity_type_specific_predicate_token(&predicate.type_specific);
    let unsupported = predicate.unsupported;
    quote! {
        EntityPredicate {
            entity_type: #entity_type,
            vehicle: #vehicle,
            flags: #flags,
            type_specific: #type_specific,
            unsupported: #unsupported,
        }
    }
}

fn damage_source_predicate_token(predicate: &DamageSourcePredicateJson) -> TokenStream {
    let tags = predicate.tags.iter().map(|tag| {
        let tag_id = identifier_token(&tag.tag);
        let expected = tag.expected;
        quote! {
            DamageSourceTagPredicate {
                tag: #tag_id,
                expected: #expected,
            }
        }
    });
    let is_direct = if let Some(is_direct) = predicate.is_direct {
        quote! { Some(#is_direct) }
    } else {
        quote! { None }
    };

    quote! { DamageSourcePredicate { tags: &[#(#tags),*], is_direct: #is_direct } }
}

fn enchantment_target_token(target: &EnchantmentTargetJson) -> TokenStream {
    match target {
        EnchantmentTargetJson::Attacker => quote! { EnchantmentTarget::Attacker },
        EnchantmentTargetJson::DamagingEntity => quote! { EnchantmentTarget::DamagingEntity },
        EnchantmentTargetJson::Victim => quote! { EnchantmentTarget::Victim },
    }
}

fn mob_effect_selection_token(selection: &MobEffectSelectionJson) -> TokenStream {
    match selection {
        MobEffectSelectionJson::Single(effect) => {
            let effect = mob_effect_ref_token(effect);
            quote! { MobEffectSelection::Single(#effect) }
        }
        MobEffectSelectionJson::UnsupportedTag(tag) => {
            let tag = identifier_token(tag);
            quote! { MobEffectSelection::UnsupportedTag(#tag) }
        }
    }
}

fn generate_entity_effect_ref(
    prefix: &str,
    effect: &EntityEffectJson,
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    let ident = Ident::new(
        &format!("{prefix}_ENTITY_EFFECT_{}", *counter),
        Span::call_site(),
    );
    *counter += 1;
    let effect = generate_entity_effect(prefix, effect, statics, counter);

    statics.extend(quote! {
        static #ident: EnchantmentEntityEffect = #effect;
    });

    quote! { &#ident }
}

fn generate_entity_effect(
    prefix: &str,
    effect: &EntityEffectJson,
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    match effect {
        EntityEffectJson::AllOf(effects) => {
            let effects = effects
                .iter()
                .map(|effect| generate_entity_effect_ref(prefix, effect, statics, counter));
            quote! { EnchantmentEntityEffect::AllOf(&[#(#effects),*]) }
        }
        EntityEffectJson::ChangeItemDamage { amount } => {
            let amount = generate_level_based_value_ref(prefix, amount, statics, counter);
            quote! { EnchantmentEntityEffect::ChangeItemDamage { amount: #amount } }
        }
        EntityEffectJson::ApplyExhaustion { amount } => {
            let amount = generate_level_based_value_ref(prefix, amount, statics, counter);
            quote! { EnchantmentEntityEffect::ApplyExhaustion { amount: #amount } }
        }
        EntityEffectJson::ApplyImpulse {
            direction,
            coordinate_scale,
            magnitude,
        } => {
            let [direction_x, direction_y, direction_z] = *direction;
            let [scale_x, scale_y, scale_z] = *coordinate_scale;
            let magnitude = generate_level_based_value_ref(prefix, magnitude, statics, counter);
            quote! {
                EnchantmentEntityEffect::ApplyImpulse {
                    direction: DVec3::new(#direction_x, #direction_y, #direction_z),
                    coordinate_scale: DVec3::new(#scale_x, #scale_y, #scale_z),
                    magnitude: #magnitude,
                }
            }
        }
        EntityEffectJson::PlaySound {
            sounds,
            volume,
            pitch,
        } => {
            let sounds = sounds.iter().map(generate_sound_event_ref);
            quote! {
                EnchantmentEntityEffect::PlaySound {
                    sounds: &[#(#sounds),*],
                    volume: #volume,
                    pitch: #pitch,
                }
            }
        }
        EntityEffectJson::DamageEntity {
            min_damage,
            max_damage,
            damage_type,
        } => {
            let min_damage = generate_level_based_value_ref(prefix, min_damage, statics, counter);
            let max_damage = generate_level_based_value_ref(prefix, max_damage, statics, counter);
            let damage_type = damage_type_ref_token(damage_type);
            quote! {
                EnchantmentEntityEffect::DamageEntity {
                    min_damage: #min_damage,
                    max_damage: #max_damage,
                    damage_type: #damage_type,
                }
            }
        }
        EntityEffectJson::Ignite { duration } => {
            let duration = generate_level_based_value_ref(prefix, duration, statics, counter);
            quote! { EnchantmentEntityEffect::Ignite { duration: #duration } }
        }
        EntityEffectJson::ApplyMobEffect {
            to_apply,
            min_duration,
            max_duration,
            min_amplifier,
            max_amplifier,
        } => {
            let to_apply = mob_effect_selection_token(to_apply);
            let min_duration =
                generate_level_based_value_ref(prefix, min_duration, statics, counter);
            let max_duration =
                generate_level_based_value_ref(prefix, max_duration, statics, counter);
            let min_amplifier =
                generate_level_based_value_ref(prefix, min_amplifier, statics, counter);
            let max_amplifier =
                generate_level_based_value_ref(prefix, max_amplifier, statics, counter);
            quote! {
                EnchantmentEntityEffect::ApplyMobEffect {
                    to_apply: #to_apply,
                    min_duration: #min_duration,
                    max_duration: #max_duration,
                    min_amplifier: #min_amplifier,
                    max_amplifier: #max_amplifier,
                }
            }
        }
        EntityEffectJson::Unsupported { effect_type } => {
            let effect_type = identifier_token(effect_type);
            quote! { EnchantmentEntityEffect::Unsupported { effect_type: #effect_type } }
        }
    }
}

fn generate_requirements_ref(
    prefix: &str,
    requirements: &RequirementsJson,
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    let ident = Ident::new(
        &format!("{prefix}_REQUIREMENTS_{}", *counter),
        Span::call_site(),
    );
    *counter += 1;
    let requirements = generate_requirements_value(prefix, requirements, statics, counter);

    statics.extend(quote! {
        static #ident: EnchantmentEffectRequirements = #requirements;
    });

    quote! { &#ident }
}

fn generate_requirements_value(
    prefix: &str,
    requirements: &RequirementsJson,
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    match requirements {
        RequirementsJson::AllOf(terms) => {
            let terms = terms
                .iter()
                .map(|term| generate_requirements_ref(prefix, term, statics, counter));
            quote! { EnchantmentEffectRequirements::AllOf(&[#(#terms),*]) }
        }
        RequirementsJson::AnyOf(terms) => {
            let terms = terms
                .iter()
                .map(|term| generate_requirements_ref(prefix, term, statics, counter));
            quote! { EnchantmentEffectRequirements::AnyOf(&[#(#terms),*]) }
        }
        RequirementsJson::Inverted(term) => {
            let term = generate_requirements_ref(prefix, term, statics, counter);
            quote! { EnchantmentEffectRequirements::Inverted(#term) }
        }
        RequirementsJson::EntityProperties { entity, predicate } => {
            let entity = entity_target_token(entity);
            let predicate = entity_predicate_token(predicate);
            quote! {
                EnchantmentEffectRequirements::EntityProperties {
                    entity: #entity,
                    predicate: #predicate,
                }
            }
        }
        RequirementsJson::DamageSourceProperties(predicate) => {
            let predicate = damage_source_predicate_token(predicate);
            quote! { EnchantmentEffectRequirements::DamageSourceProperties(#predicate) }
        }
        RequirementsJson::RandomChance { chance } => {
            let chance = generate_level_based_value_ref(prefix, chance, statics, counter);
            quote! { EnchantmentEffectRequirements::RandomChance { chance: #chance } }
        }
        RequirementsJson::MatchTool { items } => {
            let items = if let Some(items) = items {
                let items = match items {
                    ItemHolderSetJson::Tag(tag) => {
                        let tag = identifier_token(tag);
                        quote! { EnchantmentItemSet::Tag(#tag) }
                    }
                    ItemHolderSetJson::Direct(items) => {
                        let items = items.iter().map(identifier_token);
                        quote! { EnchantmentItemSet::Direct(&[#(#items),*]) }
                    }
                };
                quote! { Some(#items) }
            } else {
                quote! { None }
            };
            quote! { EnchantmentEffectRequirements::MatchTool { items: #items } }
        }
        RequirementsJson::Unsupported { condition } => {
            let condition = identifier_token(condition);
            quote! { EnchantmentEffectRequirements::Unsupported { condition: #condition } }
        }
    }
}

fn generate_optional_requirements(
    prefix: &str,
    requirements: &Option<RequirementsJson>,
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    if let Some(requirements) = requirements {
        let requirements = generate_requirements_ref(prefix, requirements, statics, counter);
        quote! { Some(#requirements) }
    } else {
        quote! { None }
    }
}

fn generate_conditional_value_effects(
    prefix: &str,
    effects: &[ConditionalValueEffectJson],
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    let entries = effects.iter().enumerate().map(|(index, effect)| {
        let entry_prefix = format!("{prefix}_{index}");
        let effect_token = generate_value_effect(&entry_prefix, &effect.effect, statics, counter);
        let requirements =
            generate_optional_requirements(&entry_prefix, &effect.requirements, statics, counter);
        quote! {
            ConditionalEnchantmentEffect {
                effect: #effect_token,
                requirements: #requirements,
            }
        }
    });

    quote! { &[#(#entries),*] }
}

fn generate_conditional_entity_effects(
    prefix: &str,
    effects: &[ConditionalEntityEffectJson],
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    let entries = effects.iter().enumerate().map(|(index, effect)| {
        let entry_prefix = format!("{prefix}_{index}");
        let effect_token = generate_entity_effect(&entry_prefix, &effect.effect, statics, counter);
        let requirements =
            generate_optional_requirements(&entry_prefix, &effect.requirements, statics, counter);
        quote! {
            ConditionalEnchantmentEffect {
                effect: #effect_token,
                requirements: #requirements,
            }
        }
    });

    quote! { &[#(#entries),*] }
}

fn generate_damage_immunity_effects(
    prefix: &str,
    effects: &[ConditionalDamageImmunityEffectJson],
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    let entries = effects.iter().enumerate().map(|(index, effect)| {
        let entry_prefix = format!("{prefix}_{index}");
        let requirements =
            generate_optional_requirements(&entry_prefix, &effect.requirements, statics, counter);
        quote! {
            ConditionalDamageImmunityEffect {
                requirements: #requirements,
            }
        }
    });

    quote! { &[#(#entries),*] }
}

fn generate_targeted_entity_effects(
    prefix: &str,
    effects: &[TargetedConditionalEntityEffectJson],
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    let entries = effects.iter().enumerate().map(|(index, effect)| {
        let entry_prefix = format!("{prefix}_{index}");
        let effect_token = generate_entity_effect(&entry_prefix, &effect.effect, statics, counter);
        let enchanted = enchantment_target_token(&effect.enchanted);
        let affected = enchantment_target_token(&effect.affected);
        let requirements =
            generate_optional_requirements(&entry_prefix, &effect.requirements, statics, counter);
        quote! {
            TargetedConditionalEnchantmentEffect {
                effect: #effect_token,
                enchanted: #enchanted,
                affected: #affected,
                requirements: #requirements,
            }
        }
    });

    quote! { &[#(#entries),*] }
}

fn generate_targeted_value_effects(
    prefix: &str,
    effects: &[TargetedConditionalValueEffectJson],
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    let entries = effects.iter().enumerate().map(|(index, effect)| {
        let entry_prefix = format!("{prefix}_{index}");
        let effect_token = generate_value_effect(&entry_prefix, &effect.effect, statics, counter);
        let enchanted = enchantment_target_token(&effect.enchanted);
        let requirements =
            generate_optional_requirements(&entry_prefix, &effect.requirements, statics, counter);
        quote! {
            TargetedConditionalEnchantmentEffect {
                effect: #effect_token,
                enchanted: #enchanted,
                affected: EnchantmentTarget::Victim,
                requirements: #requirements,
            }
        }
    });

    quote! { &[#(#entries),*] }
}

fn generate_attribute_effects(
    prefix: &str,
    attributes: &[AttributeEffectJson],
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    let entries = attributes.iter().enumerate().map(|(index, effect)| {
        let entry_prefix = format!("{prefix}_ATTRIBUTE_{index}");
        let amount =
            generate_level_based_value_ref(&entry_prefix, &effect.amount, statics, counter);
        let attribute = attribute_ref_token(&effect.attribute);
        let id = identifier_token(&effect.id);
        let operation = attribute_modifier_operation_token(&effect.operation);
        quote! {
            EnchantmentAttributeEffect {
                amount: #amount,
                attribute: #attribute,
                id: #id,
                operation: #operation,
            }
        }
    });

    quote! { &[#(#entries),*] }
}

fn generate_optional_value_effect(
    prefix: &str,
    effect: &Option<ValueEffectJson>,
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    if let Some(effect) = effect {
        let effect = generate_value_effect(prefix, effect, statics, counter);
        quote! { Some(#effect) }
    } else {
        quote! { None }
    }
}

fn generate_crossbow_charging_sounds(sounds: &[CrossbowChargingSoundsJson]) -> TokenStream {
    let entries = sounds.iter().map(|sounds| {
        let start = option_sound_event_ref_token(sounds.start.as_ref());
        let mid = option_sound_event_ref_token(sounds.mid.as_ref());
        let end = option_sound_event_ref_token(sounds.end.as_ref());
        quote! {
            CrossbowChargingSounds {
                start: #start,
                mid: #mid,
                end: #end,
            }
        }
    });

    quote! { &[#(#entries),*] }
}

fn generate_sound_event_refs(sounds: &[Identifier]) -> TokenStream {
    let sounds = sounds.iter().map(generate_sound_event_ref);
    quote! { &[#(#sounds),*] }
}

#[derive(Clone, Copy)]
enum NbtNumberHint {
    Infer,
    Float,
    Double,
}

#[derive(Clone, Copy)]
enum NbtValueHint {
    Infer,
    Float,
    Double,
    LevelBasedValue,
    FloatProvider,
    DoubleBounds,
    MovementPredicate,
}

impl NbtValueHint {
    const fn number_hint(self) -> NbtNumberHint {
        match self {
            Self::Float | Self::LevelBasedValue | Self::FloatProvider => NbtNumberHint::Float,
            Self::Double | Self::DoubleBounds => NbtNumberHint::Double,
            Self::Infer | Self::MovementPredicate => NbtNumberHint::Infer,
        }
    }
}

fn generate_nbt_number(number: &serde_json::Number, hint: NbtNumberHint) -> TokenStream {
    match hint {
        NbtNumberHint::Float => {
            let Some(value) = number.as_f64() else {
                panic!("unsupported enchantment effect NBT float: {number}");
            };
            let value = Literal::f32_unsuffixed(value as f32);
            return quote! { NbtTag::Float(#value) };
        }
        NbtNumberHint::Double => {
            let Some(value) = number.as_f64() else {
                panic!("unsupported enchantment effect NBT double: {number}");
            };
            let value = Literal::f64_unsuffixed(value);
            return quote! { NbtTag::Double(#value) };
        }
        NbtNumberHint::Infer => {}
    }

    if let Some(value) = number.as_i64() {
        if let Ok(value) = i32::try_from(value) {
            let value = Literal::i32_unsuffixed(value);
            return quote! { NbtTag::Int(#value) };
        }

        let value = Literal::i64_unsuffixed(value);
        return quote! { NbtTag::Long(#value) };
    }

    if let Some(value) = number.as_u64() {
        if let Ok(value) = i32::try_from(value) {
            let value = Literal::i32_unsuffixed(value);
            return quote! { NbtTag::Int(#value) };
        }
        if let Ok(value) = i64::try_from(value) {
            let value = Literal::i64_unsuffixed(value);
            return quote! { NbtTag::Long(#value) };
        }

        panic!("enchantment effect NBT integer out of i64 range: {value}");
    }

    let Some(value) = number.as_f64() else {
        panic!("unsupported enchantment effect NBT number: {number}");
    };
    let value = Literal::f32_unsuffixed(value as f32);
    quote! { NbtTag::Float(#value) }
}

fn generate_nbt_compound(
    value: &serde_json::Value,
    context: &str,
    hint: NbtValueHint,
) -> TokenStream {
    let Some(object) = value.as_object() else {
        panic!("enchantment effect NBT {context} must be an object");
    };
    let object_type = object.get("type").and_then(serde_json::Value::as_str);
    let entries = object.iter().map(|(key, value)| {
        let value_hint = nbt_child_value_hint(hint, object_type, key);
        let value = generate_nbt_tag(value, value_hint);
        quote! {
            compound.insert(#key, #value);
        }
    });

    quote! {{
        let mut compound = NbtCompound::new();
        #(#entries)*
        compound
    }}
}

fn nbt_child_value_hint(
    parent: NbtValueHint,
    object_type: Option<&str>,
    key: &str,
) -> NbtValueHint {
    match parent {
        NbtValueHint::LevelBasedValue => match key {
            "value" | "base" | "power" | "numerator" | "denominator" | "fallback" => {
                NbtValueHint::LevelBasedValue
            }
            "min" | "max" | "added" | "per_level_above_first" | "values" => NbtValueHint::Float,
            _ => NbtValueHint::Infer,
        },
        NbtValueHint::FloatProvider => match key {
            "value" | "min" | "max" | "min_inclusive" | "max_exclusive" | "mean" | "deviation"
            | "plateau" | "constant" | "scale" => NbtValueHint::Float,
            _ => NbtValueHint::Infer,
        },
        NbtValueHint::DoubleBounds => match key {
            "min" | "max" => NbtValueHint::Double,
            _ => NbtValueHint::Infer,
        },
        NbtValueHint::MovementPredicate => match key {
            "x" | "y" | "z" | "speed" | "horizontal_speed" | "vertical_speed" | "fall_distance" => {
                NbtValueHint::DoubleBounds
            }
            _ => NbtValueHint::Infer,
        },
        NbtValueHint::Infer | NbtValueHint::Float | NbtValueHint::Double => {
            nbt_object_child_hint(object_type, key)
        }
    }
}

fn nbt_object_child_hint(object_type: Option<&str>, key: &str) -> NbtValueHint {
    match object_type {
        Some("minecraft:apply_impulse") => match key {
            "direction" | "coordinate_scale" => NbtValueHint::Double,
            "magnitude" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:explode") => match key {
            "offset" => NbtValueHint::Double,
            "radius" | "knockback_multiplier" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:change_item_damage" | "minecraft:apply_exhaustion") => match key {
            "amount" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:damage_entity") => match key {
            "min_damage" | "max_damage" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:ignite") => match key {
            "duration" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:apply_mob_effect") => match key {
            "min_duration" | "max_duration" | "min_amplifier" | "max_amplifier" => {
                NbtValueHint::LevelBasedValue
            }
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:add" | "minecraft:set") => match key {
            "value" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:multiply") => match key {
            "factor" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:remove_binomial") => match key {
            "chance" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:play_sound") => match key {
            "volume" | "pitch" => NbtValueHint::FloatProvider,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:spawn_particles") => match key {
            "speed" => NbtValueHint::FloatProvider,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:replace_disk") => match key {
            "radius" | "height" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:clamped") => match key {
            "value" => NbtValueHint::LevelBasedValue,
            "min" | "max" => NbtValueHint::Float,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:exponent") => match key {
            "base" | "power" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:fraction") => match key {
            "numerator" | "denominator" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:levels_squared") => match key {
            "added" => NbtValueHint::Float,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:linear") => match key {
            "base" | "per_level_above_first" => NbtValueHint::Float,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:lookup") => match key {
            "values" => NbtValueHint::Float,
            "fallback" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        _ => match key {
            "minecraft:movement" | "movement" => NbtValueHint::MovementPredicate,
            "offset" | "scale" | "movement_scale" if is_float_provider_object(object_type) => {
                NbtValueHint::FloatProvider
            }
            _ => NbtValueHint::Infer,
        },
    }
}

fn is_float_provider_object(object_type: Option<&str>) -> bool {
    matches!(
        object_type,
        Some(
            "minecraft:constant"
                | "minecraft:uniform"
                | "minecraft:clamped_normal"
                | "minecraft:trapezoid"
                | "minecraft:in_bounding_box"
                | "minecraft:entity_position"
        )
    )
}

fn generate_nbt_tag(value: &serde_json::Value, hint: NbtValueHint) -> TokenStream {
    match value {
        serde_json::Value::Null => {
            panic!("enchantment effect NBT cannot contain null values");
        }
        serde_json::Value::Bool(value) => {
            let value = i8::from(*value);
            quote! { NbtTag::Byte(#value) }
        }
        serde_json::Value::Number(number) => generate_nbt_number(number, hint.number_hint()),
        serde_json::Value::String(value) => quote! { NbtTag::String(#value.into()) },
        serde_json::Value::Array(values) => {
            if values.is_empty() {
                return quote! { NbtTag::List(NbtList::Empty) };
            }

            let values = values.iter().map(|value| generate_nbt_tag(value, hint));
            quote! { NbtTag::List(NbtList::from(vec![#(#values),*])) }
        }
        serde_json::Value::Object(_) => {
            let value = generate_nbt_compound(value, "compound", hint);
            quote! { NbtTag::Compound(#value) }
        }
    }
}

fn generate_enchantment_effects(
    name: &str,
    effects: &EnchantmentEffectsJson,
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    let prefix = name.to_shouty_snake_case();
    let damage_protection = generate_conditional_value_effects(
        &format!("{prefix}_DAMAGE_PROTECTION"),
        &effects.damage_protection,
        statics,
        counter,
    );
    let damage_immunity = generate_damage_immunity_effects(
        &format!("{prefix}_DAMAGE_IMMUNITY"),
        &effects.damage_immunity,
        statics,
        counter,
    );
    let damage = generate_conditional_value_effects(
        &format!("{prefix}_DAMAGE"),
        &effects.damage,
        statics,
        counter,
    );
    let smash_damage_per_fallen_block = generate_conditional_value_effects(
        &format!("{prefix}_SMASH_DAMAGE_PER_FALLEN_BLOCK"),
        &effects.smash_damage_per_fallen_block,
        statics,
        counter,
    );
    let knockback = generate_conditional_value_effects(
        &format!("{prefix}_KNOCKBACK"),
        &effects.knockback,
        statics,
        counter,
    );
    let armor_effectiveness = generate_conditional_value_effects(
        &format!("{prefix}_ARMOR_EFFECTIVENESS"),
        &effects.armor_effectiveness,
        statics,
        counter,
    );
    let post_attack = generate_targeted_entity_effects(
        &format!("{prefix}_POST_ATTACK"),
        &effects.post_attack,
        statics,
        counter,
    );
    let post_piercing_attack = generate_conditional_entity_effects(
        &format!("{prefix}_POST_PIERCING_ATTACK"),
        &effects.post_piercing_attack,
        statics,
        counter,
    );
    let item_damage = generate_conditional_value_effects(
        &format!("{prefix}_ITEM_DAMAGE"),
        &effects.item_damage,
        statics,
        counter,
    );
    let equipment_drops = generate_targeted_value_effects(
        &format!("{prefix}_EQUIPMENT_DROPS"),
        &effects.equipment_drops,
        statics,
        counter,
    );
    let ammo_use = generate_conditional_value_effects(
        &format!("{prefix}_AMMO_USE"),
        &effects.ammo_use,
        statics,
        counter,
    );
    let projectile_piercing = generate_conditional_value_effects(
        &format!("{prefix}_PROJECTILE_PIERCING"),
        &effects.projectile_piercing,
        statics,
        counter,
    );
    let projectile_spawned = generate_conditional_entity_effects(
        &format!("{prefix}_PROJECTILE_SPAWNED"),
        &effects.projectile_spawned,
        statics,
        counter,
    );
    let projectile_spread = generate_conditional_value_effects(
        &format!("{prefix}_PROJECTILE_SPREAD"),
        &effects.projectile_spread,
        statics,
        counter,
    );
    let projectile_count = generate_conditional_value_effects(
        &format!("{prefix}_PROJECTILE_COUNT"),
        &effects.projectile_count,
        statics,
        counter,
    );
    let trident_return_acceleration = generate_conditional_value_effects(
        &format!("{prefix}_TRIDENT_RETURN_ACCELERATION"),
        &effects.trident_return_acceleration,
        statics,
        counter,
    );
    let fishing_time_reduction = generate_conditional_value_effects(
        &format!("{prefix}_FISHING_TIME_REDUCTION"),
        &effects.fishing_time_reduction,
        statics,
        counter,
    );
    let fishing_luck_bonus = generate_conditional_value_effects(
        &format!("{prefix}_FISHING_LUCK_BONUS"),
        &effects.fishing_luck_bonus,
        statics,
        counter,
    );
    let block_experience = generate_conditional_value_effects(
        &format!("{prefix}_BLOCK_EXPERIENCE"),
        &effects.block_experience,
        statics,
        counter,
    );
    let mob_experience = generate_conditional_value_effects(
        &format!("{prefix}_MOB_EXPERIENCE"),
        &effects.mob_experience,
        statics,
        counter,
    );
    let repair_with_xp = generate_conditional_value_effects(
        &format!("{prefix}_REPAIR_WITH_XP"),
        &effects.repair_with_xp,
        statics,
        counter,
    );
    let attributes = generate_attribute_effects(
        &format!("{prefix}_ATTRIBUTES"),
        &effects.attributes,
        statics,
        counter,
    );
    let crossbow_charge_time = generate_optional_value_effect(
        &format!("{prefix}_CROSSBOW_CHARGE_TIME"),
        &effects.crossbow_charge_time,
        statics,
        counter,
    );
    let crossbow_charging_sounds =
        generate_crossbow_charging_sounds(&effects.crossbow_charging_sounds);
    let trident_sound = generate_sound_event_refs(&effects.trident_sound);
    let trident_spin_attack_strength = generate_optional_value_effect(
        &format!("{prefix}_TRIDENT_SPIN_ATTACK_STRENGTH"),
        &effects.trident_spin_attack_strength,
        statics,
        counter,
    );

    let hit_block = !effects.hit_block.is_empty();
    let location_changed = !effects.location_changed.is_empty();
    let tick = !effects.tick.is_empty();
    let prevent_equipment_drop = effects.prevent_equipment_drop.is_some();
    let prevent_armor_change = effects.prevent_armor_change.is_some();

    quote! {
        EnchantmentEffects {
            damage_protection: #damage_protection,
            damage_immunity: #damage_immunity,
            damage: #damage,
            smash_damage_per_fallen_block: #smash_damage_per_fallen_block,
            knockback: #knockback,
            armor_effectiveness: #armor_effectiveness,
            post_attack: #post_attack,
            post_piercing_attack: #post_piercing_attack,
            hit_block: #hit_block,
            item_damage: #item_damage,
            equipment_drops: #equipment_drops,
            location_changed: #location_changed,
            tick: #tick,
            ammo_use: #ammo_use,
            projectile_piercing: #projectile_piercing,
            projectile_spawned: #projectile_spawned,
            projectile_spread: #projectile_spread,
            projectile_count: #projectile_count,
            trident_return_acceleration: #trident_return_acceleration,
            fishing_time_reduction: #fishing_time_reduction,
            fishing_luck_bonus: #fishing_luck_bonus,
            block_experience: #block_experience,
            mob_experience: #mob_experience,
            repair_with_xp: #repair_with_xp,
            attributes: #attributes,
            crossbow_charge_time: #crossbow_charge_time,
            crossbow_charging_sounds: #crossbow_charging_sounds,
            trident_sound: #trident_sound,
            prevent_equipment_drop: #prevent_equipment_drop,
            prevent_armor_change: #prevent_armor_change,
            trident_spin_attack_strength: #trident_spin_attack_strength,
        }
    }
}

pub(crate) fn build() -> TokenStream {
    let enchantment_dir = "../steel-utils/build_assets/builtin_datapacks/minecraft/enchantment";
    println!("cargo:rerun-if-changed={enchantment_dir}");
    let mut enchantments = Vec::new();

    for entry in fs::read_dir(enchantment_dir).expect("Failed to read enchantment directory") {
        let entry = entry.expect("Failed to read directory entry");
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }

        let name = path
            .file_stem()
            .expect("No file stem")
            .to_str()
            .expect("Invalid UTF-8")
            .to_string();
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
        let raw_enchantment: serde_json::Value = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse raw enchantment {name}: {e}"));
        let effects_nbt = raw_enchantment
            .get("effects")
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
        let ench: EnchantmentJson = serde_json::from_value(raw_enchantment)
            .unwrap_or_else(|e| panic!("Failed to parse {name}: {e}"));

        enchantments.push((name, ench, effects_nbt));
    }

    enchantments.sort_by(|a, b| a.0.cmp(&b.0));

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use glam::DVec3;
        use crate::attribute::AttributeModifierOperation;
        use crate::enchantment_effect::{
            ConditionalDamageImmunityEffect, ConditionalEnchantmentEffect,
            CrossbowChargingSounds, DamageSourcePredicate, DamageSourceTagPredicate,
            EnchantmentAttributeEffect, EnchantmentEffectRequirements, EnchantmentEffects,
            EnchantmentEntityEffect, EnchantmentEntityTarget, EnchantmentItemSet, EnchantmentTarget,
            EnchantmentValueEffect, EntityFlagsPredicate, EntityPredicate,
            EntityTypePredicate, EntityTypeSpecificPredicate, EntityVehiclePredicate,
            LevelBasedValue, MobEffectSelection, PlayerPredicate,
            TargetedConditionalEnchantmentEffect,
        };
        use crate::enchantment::{Enchantment, EnchantmentCost, EnchantmentRegistry};
        use crate::equipment::EquipmentSlotGroup;
        use crate::vanilla_attributes;
        use crate::vanilla_mob_effects;
        use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
        use steel_utils::Identifier;
        use steel_utils::types::GameType;
    });

    let mut register_stream = TokenStream::new();
    let mut value_statics = TokenStream::new();
    let mut value_static_counter = 0;

    for (name, ench, effects_nbt) in &enchantments {
        let const_ident = Ident::new(&name.to_shouty_snake_case(), Span::call_site());
        let effects_nbt_fn_ident = Ident::new(
            &format!("{}_effects_nbt", name.to_snake_case()),
            Span::call_site(),
        );

        let max_level = Literal::u32_unsuffixed(ench.max_level);
        let min_cost_base = Literal::i32_unsuffixed(ench.min_cost.base);
        let min_cost_per = Literal::i32_unsuffixed(ench.min_cost.per_level_above_first);
        let max_cost_base = Literal::i32_unsuffixed(ench.max_cost.base);
        let max_cost_per = Literal::i32_unsuffixed(ench.max_cost.per_level_above_first);
        let anvil_cost = Literal::i32_unsuffixed(ench.anvil_cost);
        let weight = Literal::u32_unsuffixed(ench.weight);

        let slots: Vec<TokenStream> = ench.slots.iter().map(|s| slot_to_tokens(s)).collect();

        let supported_items = ench.supported_items.as_str();
        let primary_items = if let Some(s) = &ench.primary_items {
            let s = s.as_str();
            quote! { Some(#s) }
        } else {
            quote! { None }
        };
        let exclusive_set = if let Some(s) = &ench.exclusive_set {
            let s = s.as_str();
            quote! { Some(#s) }
        } else {
            quote! { None }
        };
        let effects = generate_enchantment_effects(
            name,
            &ench.effects,
            &mut value_statics,
            &mut value_static_counter,
        );
        let effects_nbt = generate_nbt_compound(effects_nbt, "effects", NbtValueHint::Infer);

        stream.extend(quote! {
            fn #effects_nbt_fn_ident() -> NbtCompound {
                #effects_nbt
            }

            pub static #const_ident: Enchantment = Enchantment {
                key: Identifier::vanilla_static(#name),
                max_level: #max_level,
                min_cost: EnchantmentCost { base: #min_cost_base, per_level_above_first: #min_cost_per },
                max_cost: EnchantmentCost { base: #max_cost_base, per_level_above_first: #max_cost_per },
                anvil_cost: #anvil_cost,
                weight: #weight,
                slots: &[#(#slots),*],
                supported_items: #supported_items,
                primary_items: #primary_items,
                exclusive_set: #exclusive_set,
                effects_nbt: #effects_nbt_fn_ident,
                effects: #effects,
            };
        });

        register_stream.extend(quote! {
            registry.register(&#const_ident);
        });
    }

    stream.extend(quote! {
        #value_statics

        pub fn register_enchantments(registry: &mut EnchantmentRegistry) {
            #register_stream
        }
    });

    stream
}
