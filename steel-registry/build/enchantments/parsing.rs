use super::{
    DamageSourcePredicateJson, DamageSourceTagPredicateJson, DoubleBoundsJson,
    EnchantmentTargetJson, EntityEffectJson, EntityFlagsPredicateJson, EntityPredicateJson,
    EntityTargetJson, EntityTypePredicateJson, EntityTypeSpecificPredicateJson,
    EntityVehiclePredicateJson, ExplodeEffectJson, ExplosionInteractionJson, GameType, Ident,
    Identifier, ItemHolderSetJson, LevelBasedValueJson, MobEffectSelectionJson,
    MovementPredicateJson, PlayerPredicateJson, RequirementsJson, Span, TokenStream, quote,
};

pub(super) fn slot_to_tokens(slot: &str) -> TokenStream {
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

pub(super) fn identifier_token(identifier: &Identifier) -> TokenStream {
    let namespace = identifier.namespace.as_ref();
    let path = identifier.path.as_ref();
    quote! { Identifier::new_static(#namespace, #path) }
}

pub(super) fn damage_type_ref_token(identifier: &Identifier) -> TokenStream {
    assert_eq!(
        identifier.namespace.as_ref(),
        "minecraft",
        "vanilla enchantment damage_type references must use the minecraft namespace: {identifier}"
    );
    let ident = Ident::new(&identifier.path.to_ascii_uppercase(), Span::call_site());
    quote! { &crate::vanilla_damage_types::#ident }
}

pub(super) fn parse_identifier(raw: &str) -> Result<Identifier, String> {
    raw.parse::<Identifier>()
        .map_err(|error| format!("invalid identifier {raw}: {error}"))
}

/// Parses a value that must be a `#`-prefixed tag reference (as opposed to the
/// tag-or-direct-id shapes handled inline elsewhere in this module).
pub(super) fn parse_required_tag(raw: &str) -> Result<Identifier, String> {
    let tag = raw
        .strip_prefix('#')
        .ok_or_else(|| format!("expected a tag reference (`#...`), got {raw}"))?;
    parse_identifier(tag)
}

pub(super) fn object_field<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<&'a serde_json::Value, String> {
    object
        .get(field)
        .ok_or_else(|| format!("missing enchantment requirement field `{field}`"))
}

pub(super) fn string_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<String, String> {
    object_field(object, field)?
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("enchantment requirement field `{field}` must be a string"))
}

pub(super) fn parse_entity_target(raw: &str) -> Result<EntityTargetJson, String> {
    match raw {
        "this" => Ok(EntityTargetJson::This),
        "attacker" => Ok(EntityTargetJson::Attacker),
        "direct_attacker" => Ok(EntityTargetJson::DirectAttacker),
        other => Err(format!("unsupported enchantment entity target `{other}`")),
    }
}

pub(super) fn parse_enchantment_target(raw: &str) -> Result<EnchantmentTargetJson, String> {
    match raw {
        "attacker" => Ok(EnchantmentTargetJson::Attacker),
        "damaging_entity" => Ok(EnchantmentTargetJson::DamagingEntity),
        "victim" => Ok(EnchantmentTargetJson::Victim),
        other => Err(format!(
            "unsupported enchantment post-attack target `{other}`"
        )),
    }
}

pub(super) fn parse_level_based_value_json(
    value: &serde_json::Value,
) -> Result<LevelBasedValueJson, String> {
    serde_json::from_value(value.to_owned())
        .map_err(|error| format!("invalid level-based value: {error}"))
}

pub(super) fn parse_random_chance_value(
    value: &serde_json::Value,
) -> Result<LevelBasedValueJson, String> {
    let Some(object) = value.as_object() else {
        return parse_level_based_value_json(value);
    };
    if object.get("type").and_then(serde_json::Value::as_str) != Some("minecraft:enchantment_level")
    {
        return parse_level_based_value_json(value);
    }
    parse_level_based_value_json(object_field(object, "amount")?)
}

pub(super) fn parse_mob_effect_selection_json(
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

pub(super) fn parse_entity_effect_json(
    value: &serde_json::Value,
) -> Result<EntityEffectJson, String> {
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
        "minecraft:explode" => Ok(EntityEffectJson::Explode(Box::new(
            parse_explode_effect_json(object)?,
        ))),
        _ => Ok(EntityEffectJson::Unsupported {
            effect_type: parse_identifier(&effect_type)?,
        }),
    }
}

fn parse_explode_effect_json(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<ExplodeEffectJson, String> {
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "type"
                | "attribute_to_user"
                | "damage_type"
                | "knockback_multiplier"
                | "immune_blocks"
                | "offset"
                | "radius"
                | "create_fire"
                | "block_interaction"
                | "small_particle"
                | "large_particle"
                | "sound"
        ) {
            // Includes `block_particles`: Steel only supports vanilla's empty default.
            return Err(format!("unsupported explode effect field `{key}`"));
        }
    }

    let immune_blocks_tag = match object.get("immune_blocks") {
        Some(serde_json::Value::String(raw)) => Some(parse_required_tag(raw).map_err(|error| {
            format!("explode `immune_blocks` must be a block tag (direct block lists are unsupported): {error}")
        })?),
        Some(_) => {
            return Err(
                "explode `immune_blocks` must be a block tag (direct block lists are unsupported)"
                    .to_owned(),
            );
        }
        None => None,
    };

    Ok(ExplodeEffectJson {
        attribute_to_user: optional_bool_field(object, "attribute_to_user")?.unwrap_or(false),
        damage_type: object
            .get("damage_type")
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| "explode `damage_type` must be a string".to_owned())
                    .and_then(parse_identifier)
            })
            .transpose()?,
        knockback_multiplier: object
            .get("knockback_multiplier")
            .map(parse_level_based_value_json)
            .transpose()?,
        immune_blocks_tag,
        offset: object
            .get("offset")
            .map(parse_vec3_json)
            .transpose()?
            .unwrap_or([0.0; 3]),
        radius: parse_level_based_value_json(object_field(object, "radius")?)?,
        create_fire: optional_bool_field(object, "create_fire")?.unwrap_or(false),
        block_interaction: parse_explosion_interaction(&string_field(
            object,
            "block_interaction",
        )?)?,
        small_particle: parse_simple_particle_json(object_field(object, "small_particle")?)?,
        large_particle: parse_simple_particle_json(object_field(object, "large_particle")?)?,
        sound: object_field(object, "sound")?
            .as_str()
            .ok_or_else(|| {
                "explode `sound` must be a sound event id (inline sound events are unsupported)"
                    .to_owned()
            })
            .and_then(parse_identifier)?,
    })
}

fn parse_explosion_interaction(raw: &str) -> Result<ExplosionInteractionJson, String> {
    match raw {
        "none" => Ok(ExplosionInteractionJson::None),
        "block" => Ok(ExplosionInteractionJson::Block),
        "mob" => Ok(ExplosionInteractionJson::Mob),
        "tnt" => Ok(ExplosionInteractionJson::Tnt),
        "trigger" => Ok(ExplosionInteractionJson::Trigger),
        other => Err(format!("unknown explosion block_interaction `{other}`")),
    }
}

/// Parses a particle that carries no options beyond its type.
fn parse_simple_particle_json(value: &serde_json::Value) -> Result<Identifier, String> {
    let Some(object) = value.as_object() else {
        return Err("particle must be an object".to_owned());
    };
    if let Some(key) = object.keys().find(|key| *key != "type") {
        return Err(format!(
            "particle options are unsupported in enchantment effects (field `{key}`)"
        ));
    }
    parse_identifier(&string_field(object, "type")?)
}

pub(super) fn parse_vec3_json(value: &serde_json::Value) -> Result<[f64; 3], String> {
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

pub(super) fn parse_sound_list_json(value: &serde_json::Value) -> Result<Vec<Identifier>, String> {
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

pub(super) fn parse_f32_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<f32, String> {
    let value = object_field(object, field)?;
    let number = value
        .as_f64()
        .ok_or_else(|| format!("`{field}` must be a number"))?;
    Ok(number as f32)
}

pub(super) fn parse_entity_type_predicate(raw: &str) -> Result<EntityTypePredicateJson, String> {
    let Some(tag) = raw.strip_prefix('#') else {
        return Ok(EntityTypePredicateJson::Type(parse_identifier(raw)?));
    };

    Ok(EntityTypePredicateJson::Tag(parse_identifier(tag)?))
}

pub(super) fn parse_entity_predicate_json(
    value: &serde_json::Value,
) -> Result<EntityPredicateJson, String> {
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
                | "movement"
                | "minecraft:movement"
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
    let movement = aliased_object_field(object, &["movement", "minecraft:movement"])?
        .map(parse_movement_predicate_json)
        .transpose()?
        .map(Box::new);
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
        movement,
        type_specific,
        unsupported,
    })
}

fn parse_movement_predicate_json(
    value: &serde_json::Value,
) -> Result<MovementPredicateJson, String> {
    let Some(object) = value.as_object() else {
        return Err("movement predicate must be an object".to_owned());
    };
    let mut movement = MovementPredicateJson::default();
    for (key, value) in object {
        let bounds = match key.as_str() {
            "x" => &mut movement.x,
            "y" => &mut movement.y,
            "z" => &mut movement.z,
            "speed" => &mut movement.speed,
            "horizontal_speed" => &mut movement.horizontal_speed,
            "vertical_speed" => &mut movement.vertical_speed,
            "fall_distance" => &mut movement.fall_distance,
            other => return Err(format!("unknown movement predicate field `{other}`")),
        };
        *bounds = parse_double_bounds_json(value)
            .map_err(|error| format!("movement predicate `{key}`: {error}"))?;
    }
    Ok(movement)
}

/// Parses vanilla `MinMaxBounds.Doubles`: a number for an exact value, or `{min, max}`.
fn parse_double_bounds_json(value: &serde_json::Value) -> Result<DoubleBoundsJson, String> {
    if let Some(exact) = value.as_f64() {
        return Ok(DoubleBoundsJson {
            min: Some(exact),
            max: Some(exact),
        });
    }
    let Some(object) = value.as_object() else {
        return Err("bounds must be a number or an object".to_owned());
    };
    let bound = |field: &str| -> Result<Option<f64>, String> {
        object
            .get(field)
            .map(|value| {
                value
                    .as_f64()
                    .ok_or_else(|| format!("bounds `{field}` must be a number"))
            })
            .transpose()
    };
    if let Some(key) = object.keys().find(|key| *key != "min" && *key != "max") {
        return Err(format!("unknown bounds field `{key}`"));
    }
    let bounds = DoubleBoundsJson {
        min: bound("min")?,
        max: bound("max")?,
    };
    if let (Some(min), Some(max)) = (bounds.min, bounds.max)
        && min > max
    {
        return Err("bounds min must not exceed max".to_owned());
    }
    Ok(bounds)
}

pub(super) fn aliased_object_field<'a>(
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

pub(super) fn parse_entity_flags_predicate_json(
    value: &serde_json::Value,
) -> Result<EntityFlagsPredicateJson, String> {
    let Some(object) = value.as_object() else {
        return Err("entity flags predicate must be an object".to_owned());
    };
    let unsupported = object
        .keys()
        .any(|key| !matches!(key.as_str(), "is_flying" | "is_fall_flying" | "is_in_water"));
    let is_flying = optional_bool_field(object, "is_flying")?;
    let is_fall_flying = optional_bool_field(object, "is_fall_flying")?;
    let is_in_water = optional_bool_field(object, "is_in_water")?;

    Ok(EntityFlagsPredicateJson {
        is_flying,
        is_fall_flying,
        is_in_water,
        unsupported,
    })
}

pub(super) fn parse_type_specific_predicate_json(
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

pub(super) fn parse_player_predicate_json(
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

pub(super) fn parse_player_food_min_json(value: &serde_json::Value) -> Result<i32, String> {
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

pub(super) fn optional_bool_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<Option<bool>, String> {
    match object.get(field) {
        Some(serde_json::Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(format!("`{field}` must be a bool")),
        None => Ok(None),
    }
}

pub(super) fn parse_game_type(value: &str) -> Result<GameType, String> {
    match value {
        "survival" => Ok(GameType::Survival),
        "creative" => Ok(GameType::Creative),
        "adventure" => Ok(GameType::Adventure),
        "spectator" => Ok(GameType::Spectator),
        other => Err(format!("unknown game type `{other}`")),
    }
}

pub(super) fn parse_damage_source_predicate_json(
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

pub(super) fn parse_damage_source_tag_predicate_json(
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

    // `tags` entries are always a damage-type tag reference (`DamageTypeTags`), written with
    // the vanilla `#` sigil like any other tag id.
    Ok(DamageSourceTagPredicateJson {
        tag: parse_required_tag(&id)?,
        expected,
    })
}

pub(super) fn parse_requirements_json(
    value: &serde_json::Value,
) -> Result<RequirementsJson, String> {
    let Some(object) = value.as_object() else {
        return Err("enchantment effect requirements must be an object".to_owned());
    };
    let condition = string_field(object, "type")?;

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
                if key != "type" && key != "chance" {
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
                if key != "type" && key != "predicate" {
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

pub(super) fn parse_item_holder_set_json(
    value: &serde_json::Value,
) -> Result<ItemHolderSetJson, String> {
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
