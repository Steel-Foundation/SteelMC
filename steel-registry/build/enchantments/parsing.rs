use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value};

use super::{
    DamageSourcePredicateJson, DamageSourceTagPredicateJson, EntityFlagsPredicateJson,
    EntityPredicateJson, EntityTargetJson, EntityTypePredicateJson,
    EntityTypeSpecificPredicateJson, EntityVehiclePredicateJson, GameType, Identifier,
    ItemHolderSetJson, LevelBasedValueJson, MobEffectSelectionJson, PlayerPredicateJson,
    RequirementsJson,
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

pub(super) fn parse_required_tag(raw: &str) -> Result<Identifier, String> {
    let tag = raw
        .strip_prefix('#')
        .ok_or_else(|| format!("expected a tag reference (#...), got {raw}"))?;
    parse_identifier(tag)
}

fn decode<T: DeserializeOwned>(value: &Value, description: &str) -> Result<T, String> {
    serde_json::from_value(value.clone())
        .map_err(|error| format!("invalid {description}: {error}"))
}

#[derive(Deserialize)]
struct TaggedObject {
    #[serde(rename = "type")]
    kind: String,
    #[serde(flatten)]
    _fields: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum EntityTargetInput {
    This,
    Attacker,
    DirectAttacker,
}

impl From<EntityTargetInput> for EntityTargetJson {
    fn from(value: EntityTargetInput) -> Self {
        match value {
            EntityTargetInput::This => Self::This,
            EntityTargetInput::Attacker => Self::Attacker,
            EntityTargetInput::DirectAttacker => Self::DirectAttacker,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum GameTypeInput {
    Survival,
    Creative,
    Adventure,
    Spectator,
}

impl From<GameTypeInput> for GameType {
    fn from(value: GameTypeInput) -> Self {
        match value {
            GameTypeInput::Survival => Self::Survival,
            GameTypeInput::Creative => Self::Creative,
            GameTypeInput::Adventure => Self::Adventure,
            GameTypeInput::Spectator => Self::Spectator,
        }
    }
}

#[derive(Deserialize)]
struct EntityPredicateInput {
    #[serde(default, alias = "minecraft:entity_type")]
    entity_type: Option<String>,
    #[serde(default, alias = "minecraft:vehicle")]
    vehicle: Option<Map<String, Value>>,
    #[serde(default, alias = "minecraft:flags")]
    flags: Option<EntityFlagsInput>,
    #[serde(default)]
    type_specific: Option<Value>,
    #[serde(default, rename = "minecraft:type_specific/player")]
    player_type_specific: Option<Value>,
    #[serde(flatten)]
    unsupported_fields: Map<String, Value>,
}

#[derive(Deserialize)]
struct EntityFlagsInput {
    #[serde(default)]
    is_fall_flying: Option<bool>,
    #[serde(default)]
    is_in_water: Option<bool>,
    #[serde(flatten)]
    unsupported_fields: Map<String, Value>,
}

#[derive(Deserialize)]
struct PlayerPredicateInput {
    #[serde(default)]
    gamemode: Vec<GameTypeInput>,
    #[serde(default)]
    food: Option<PlayerFoodInput>,
    #[serde(flatten)]
    unsupported_fields: Map<String, Value>,
}

#[derive(Deserialize)]
struct TypedPlayerPredicateInput {
    #[serde(rename = "type")]
    _kind: String,
    #[serde(default)]
    gamemode: Vec<GameTypeInput>,
    #[serde(default)]
    food: Option<PlayerFoodInput>,
    #[serde(flatten)]
    unsupported_fields: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerFoodInput {
    level: PlayerFoodLevelInput,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerFoodLevelInput {
    min: i32,
}

fn player_predicate(
    game_modes: Vec<GameTypeInput>,
    food: Option<PlayerFoodInput>,
    unsupported: bool,
) -> PlayerPredicateJson {
    PlayerPredicateJson {
        game_modes: game_modes.into_iter().map(GameType::from).collect(),
        food_level_min: food.map(|food| food.level.min),
        unsupported,
    }
}

pub(super) fn parse_entity_predicate_json(
    value: &Value,
) -> Result<EntityPredicateJson, String> {
    let input: EntityPredicateInput = decode(value, "entity_properties predicate")?;

    let entity_type = input
        .entity_type
        .as_deref()
        .map(parse_entity_type_predicate)
        .transpose()?
        .unwrap_or(EntityTypePredicateJson::Any);

    let vehicle = input.vehicle.map_or(EntityVehiclePredicateJson::Any, |vehicle| {
        if vehicle.is_empty() {
            EntityVehiclePredicateJson::Present
        } else {
            EntityVehiclePredicateJson::Unsupported
        }
    });

    let flags = input.flags.map_or_else(EntityFlagsPredicateJson::any, |flags| {
        EntityFlagsPredicateJson {
            is_fall_flying: flags.is_fall_flying,
            is_in_water: flags.is_in_water,
            unsupported: !flags.unsupported_fields.is_empty(),
        }
    });

    let type_specific = match (input.type_specific, input.player_type_specific) {
        (Some(_), Some(_)) => {
            return Err(
                "entity_properties predicate must not contain both type_specific and minecraft:type_specific/player"
                    .to_owned(),
            );
        }
        (Some(value), None) => parse_type_specific_predicate_json(&value)?,
        (None, Some(value)) => {
            let player: PlayerPredicateInput = decode(&value, "player predicate")?;
            EntityTypeSpecificPredicateJson::Player(player_predicate(
                player.gamemode,
                player.food,
                !player.unsupported_fields.is_empty(),
            ))
        }
        (None, None) => EntityTypeSpecificPredicateJson::Any,
    };

    Ok(EntityPredicateJson {
        entity_type,
        vehicle,
        flags,
        type_specific,
        unsupported: !input.unsupported_fields.is_empty(),
    })
}

fn parse_entity_type_predicate(raw: &str) -> Result<EntityTypePredicateJson, String> {
    if let Some(tag) = raw.strip_prefix('#') {
        return Ok(EntityTypePredicateJson::Tag(parse_identifier(tag)?));
    }
    Ok(EntityTypePredicateJson::Type(parse_identifier(raw)?))
}

fn parse_type_specific_predicate_json(
    value: &Value,
) -> Result<EntityTypeSpecificPredicateJson, String> {
    let tagged: TaggedObject = decode(value, "entity type_specific predicate")?;
    if tagged.kind != "minecraft:player" {
        return Ok(EntityTypeSpecificPredicateJson::Unsupported);
    }

    let player: TypedPlayerPredicateInput = decode(value, "player predicate")?;
    Ok(EntityTypeSpecificPredicateJson::Player(player_predicate(
        player.gamemode,
        player.food,
        !player.unsupported_fields.is_empty(),
    )))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DamageSourcePredicateInput {
    #[serde(default)]
    tags: Vec<DamageSourceTagPredicateInput>,
    is_direct: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DamageSourceTagPredicateInput {
    id: String,
    expected: bool,
}

pub(super) fn parse_damage_source_predicate_json(
    value: &Value,
) -> Result<DamageSourcePredicateJson, String> {
    let input: DamageSourcePredicateInput = decode(value, "damage_source_properties predicate")?;
    let tags = input
        .tags
        .into_iter()
        .map(|tag| {
            Ok(DamageSourceTagPredicateJson {
                tag: parse_required_tag(&tag.id)?,
                expected: tag.expected,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(DamageSourcePredicateJson {
        tags,
        is_direct: input.is_direct,
    })
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ItemHolderSetInput {
    Single(String),
    Direct(Vec<String>),
}

pub(super) fn parse_item_holder_set_json(
    value: &Value,
) -> Result<ItemHolderSetJson, String> {
    let input: ItemHolderSetInput = decode(value, "match_tool item set")?;
    match input {
        ItemHolderSetInput::Single(raw) => {
            if let Some(tag) = raw.strip_prefix('#') {
                Ok(ItemHolderSetJson::Tag(parse_identifier(tag)?))
            } else {
                Ok(ItemHolderSetJson::Direct(vec![parse_identifier(&raw)?]))
            }
        }
        ItemHolderSetInput::Direct(items) => {
            let items = items
                .into_iter()
                .map(|item| {
                    if item.starts_with('#') {
                        return Err(
                            "match_tool direct item lists cannot contain tags".to_owned(),
                        );
                    }
                    parse_identifier(&item)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ItemHolderSetJson::Direct(items))
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SoundListInput {
    Single(Identifier),
    Multiple(Vec<Identifier>),
}

fn parse_sound_list_json(value: SoundListInput) -> Vec<Identifier> {
    match value {
        SoundListInput::Single(sound) => vec![sound],
        SoundListInput::Multiple(sounds) => sounds,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum EnchantmentLevelType {
    EnchantmentLevel,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnchantmentLevelValueInput {
    #[serde(rename = "type")]
    _kind: EnchantmentLevelType,
    amount: LevelBasedValueJson,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RandomChanceValueInput {
    EnchantmentLevel(EnchantmentLevelValueInput),
    Value(LevelBasedValueJson),
}

fn parse_random_chance_value(value: &Value) -> Result<LevelBasedValueJson, String> {
    match decode(value, "random_chance value")? {
        RandomChanceValueInput::EnchantmentLevel(input) => Ok(input.amount),
        RandomChanceValueInput::Value(value) => Ok(value),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AllOfEffectInput {
    #[serde(rename = "type")]
    _kind: String,
    effects: Vec<super::EntityEffectJson>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeItemDamageEffectInput {
    #[serde(rename = "type")]
    _kind: String,
    amount: LevelBasedValueJson,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyExhaustionEffectInput {
    #[serde(rename = "type")]
    _kind: String,
    amount: LevelBasedValueJson,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyImpulseEffectInput {
    #[serde(rename = "type")]
    _kind: String,
    direction: [f64; 3],
    coordinate_scale: [f64; 3],
    magnitude: LevelBasedValueJson,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DamageEntityEffectInput {
    #[serde(rename = "type")]
    _kind: String,
    min_damage: LevelBasedValueJson,
    max_damage: LevelBasedValueJson,
    damage_type: Identifier,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlaySoundEffectInput {
    #[serde(rename = "type")]
    _kind: String,
    sound: SoundListInput,
    volume: f32,
    pitch: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IgniteEffectInput {
    #[serde(rename = "type")]
    _kind: String,
    duration: LevelBasedValueJson,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyMobEffectInput {
    #[serde(rename = "type")]
    _kind: String,
    to_apply: String,
    min_duration: LevelBasedValueJson,
    max_duration: LevelBasedValueJson,
    min_amplifier: LevelBasedValueJson,
    max_amplifier: LevelBasedValueJson,
}

fn parse_mob_effect_selection(raw: &str) -> Result<MobEffectSelectionJson, String> {
    if let Some(tag) = raw.strip_prefix('#') {
        return Ok(MobEffectSelectionJson::UnsupportedTag(parse_identifier(tag)?));
    }
    Ok(MobEffectSelectionJson::Single(parse_identifier(raw)?))
}

pub(super) fn parse_entity_effect_json(
    value: &Value,
) -> Result<super::EntityEffectJson, String> {
    use super::EntityEffectJson;

    let tagged: TaggedObject = decode(value, "enchantment entity effect")?;
    match tagged.kind.as_str() {
        "minecraft:all_of" => {
            let input: AllOfEffectInput = decode(value, "all_of entity effect")?;
            Ok(EntityEffectJson::AllOf(input.effects))
        }
        "minecraft:change_item_damage" => {
            let input: ChangeItemDamageEffectInput = decode(value, "change_item_damage effect")?;
            Ok(EntityEffectJson::ChangeItemDamage {
                amount: input.amount,
            })
        }
        "minecraft:apply_exhaustion" => {
            let input: ApplyExhaustionEffectInput = decode(value, "apply_exhaustion effect")?;
            Ok(EntityEffectJson::ApplyExhaustion {
                amount: input.amount,
            })
        }
        "minecraft:apply_impulse" => {
            let input: ApplyImpulseEffectInput = decode(value, "apply_impulse effect")?;
            Ok(EntityEffectJson::ApplyImpulse {
                direction: input.direction,
                coordinate_scale: input.coordinate_scale,
                magnitude: input.magnitude,
            })
        }
        "minecraft:damage_entity" => {
            let input: DamageEntityEffectInput = decode(value, "damage_entity effect")?;
            Ok(EntityEffectJson::DamageEntity {
                min_damage: input.min_damage,
                max_damage: input.max_damage,
                damage_type: input.damage_type,
            })
        }
        "minecraft:play_sound" => {
            let input: PlaySoundEffectInput = decode(value, "play_sound effect")?;
            Ok(EntityEffectJson::PlaySound {
                sounds: parse_sound_list_json(input.sound),
                volume: input.volume,
                pitch: input.pitch,
            })
        }
        "minecraft:ignite" => {
            let input: IgniteEffectInput = decode(value, "ignite effect")?;
            Ok(EntityEffectJson::Ignite {
                duration: input.duration,
            })
        }
        "minecraft:apply_mob_effect" => {
            let input: ApplyMobEffectInput = decode(value, "apply_mob_effect effect")?;
            Ok(EntityEffectJson::ApplyMobEffect {
                to_apply: parse_mob_effect_selection(&input.to_apply)?,
                min_duration: input.min_duration,
                max_duration: input.max_duration,
                min_amplifier: input.min_amplifier,
                max_amplifier: input.max_amplifier,
            })
        }
        _ => Ok(EntityEffectJson::Unsupported {
            effect_type: parse_identifier(&tagged.kind)?,
        }),
    }
}

#[derive(Deserialize)]
struct AllOfRequirementsInput {
    #[serde(rename = "type")]
    _kind: String,
    terms: Vec<RequirementsJson>,
}

#[derive(Deserialize)]
struct AnyOfRequirementsInput {
    #[serde(rename = "type")]
    _kind: String,
    terms: Vec<RequirementsJson>,
}

#[derive(Deserialize)]
struct InvertedRequirementsInput {
    #[serde(rename = "type")]
    _kind: String,
    term: Box<RequirementsJson>,
}

#[derive(Deserialize)]
struct EntityPropertiesRequirementsInput {
    #[serde(rename = "type")]
    _kind: String,
    entity: EntityTargetInput,
    predicate: Value,
}

#[derive(Deserialize)]
struct DamageSourcePropertiesRequirementsInput {
    #[serde(rename = "type")]
    _kind: String,
    predicate: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RandomChanceRequirementsInput {
    #[serde(rename = "type")]
    _kind: String,
    chance: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatchToolRequirementsInput {
    #[serde(rename = "type")]
    _kind: String,
    predicate: MatchToolPredicateInput,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatchToolPredicateInput {
    items: Option<Value>,
}

pub(super) fn parse_requirements_json(value: &Value) -> Result<RequirementsJson, String> {
    let tagged: TaggedObject = decode(value, "enchantment effect requirements")?;
    match tagged.kind.as_str() {
        "minecraft:all_of" => {
            let input: AllOfRequirementsInput = decode(value, "all_of requirements")?;
            Ok(RequirementsJson::AllOf(input.terms))
        }
        "minecraft:any_of" => {
            let input: AnyOfRequirementsInput = decode(value, "any_of requirements")?;
            Ok(RequirementsJson::AnyOf(input.terms))
        }
        "minecraft:inverted" => {
            let input: InvertedRequirementsInput = decode(value, "inverted requirements")?;
            Ok(RequirementsJson::Inverted(input.term))
        }
        "minecraft:entity_properties" => {
            let input: EntityPropertiesRequirementsInput =
                decode(value, "entity_properties requirement")?;
            Ok(RequirementsJson::EntityProperties {
                entity: input.entity.into(),
                predicate: parse_entity_predicate_json(&input.predicate)?,
            })
        }
        "minecraft:damage_source_properties" => {
            let input: DamageSourcePropertiesRequirementsInput =
                decode(value, "damage_source_properties requirement")?;
            Ok(RequirementsJson::DamageSourceProperties(
                parse_damage_source_predicate_json(&input.predicate)?,
            ))
        }
        "minecraft:random_chance" => {
            let input: RandomChanceRequirementsInput =
                decode(value, "random_chance requirement")?;
            Ok(RequirementsJson::RandomChance {
                chance: parse_random_chance_value(&input.chance)?,
            })
        }
        "minecraft:match_tool" => {
            let input: MatchToolRequirementsInput = decode(value, "match_tool requirement")?;
            let items = input
                .predicate
                .items
                .as_ref()
                .map(parse_item_holder_set_json)
                .transpose()?;
            Ok(RequirementsJson::MatchTool { items })
        }
        _ => Ok(RequirementsJson::Unsupported {
            condition: parse_identifier(&tagged.kind)?,
        }),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::EntityEffectJson;
    use super::*;

    #[test]
    fn parses_known_nested_effects_with_serde() {
        let effect: EntityEffectJson = serde_json::from_value(json!({
            "type": "minecraft:all_of",
            "effects": [
                {"type": "minecraft:ignite", "duration": 2.0},
                {
                    "type": "minecraft:apply_mob_effect",
                    "to_apply": "minecraft:glowing",
                    "min_duration": 1.0,
                    "max_duration": 2.0,
                    "min_amplifier": 0.0,
                    "max_amplifier": 1.0
                }
            ]
        }))
        .expect("known effects should deserialize");

        assert!(matches!(effect, EntityEffectJson::AllOf(effects) if effects.len() == 2));
    }

    #[test]
    fn keeps_unknown_effect_types_as_unsupported() {
        let effect: EntityEffectJson = serde_json::from_value(json!({
            "type": "minecraft:future_effect",
            "future_field": true
        }))
        .expect("unknown effect types should remain representable");

        assert!(matches!(
            effect,
            EntityEffectJson::Unsupported { effect_type }
                if effect_type.to_string() == "minecraft:future_effect"
        ));
    }

    #[test]
    fn rejects_unknown_fields_on_known_effects() {
        let effect = serde_json::from_value::<EntityEffectJson>(json!({
            "type": "minecraft:ignite",
            "duration": 2.0,
            "unexpected": true
        }));

        assert!(effect.is_err());
    }

    #[test]
    fn preserves_unknown_entity_predicate_fields_as_unsupported() {
        let predicate = parse_entity_predicate_json(&json!({
            "flags": {
                "is_fall_flying": true,
                "future_flag": true
            }
        }))
        .expect("unknown predicate fields are reported as unsupported");

        assert!(predicate.flags.unsupported);
    }

    #[test]
    fn rejects_both_entity_type_aliases() {
        let predicate = parse_entity_predicate_json(&json!({
            "type": "minecraft:pig",
            "minecraft:entity_type": "minecraft:pig"
        }));

        assert!(predicate.is_err());
    }

    #[test]
    fn preserves_unknown_condition_types_as_unsupported() {
        let condition: RequirementsJson = serde_json::from_value(json!({
            "type": "minecraft:future_condition",
            "future_field": true
        }))
        .expect("unknown condition types should remain representable");

        assert!(matches!(
            condition,
            RequirementsJson::Unsupported { condition }
                if condition.to_string() == "minecraft:future_condition"
        ));
    }
}
