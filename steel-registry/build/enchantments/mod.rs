use std::fs;

use crate::generator_functions::generate_sound_event_ref;
use heck::{ToShoutySnakeCase, ToSnakeCase};
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use serde::{Deserialize, de};
use steel_utils::Identifier;
use steel_utils::types::GameType;

mod effects;
mod generation;
mod nbt;
mod parsing;

use effects::{
    generate_attribute_effects, generate_conditional_entity_effects,
    generate_conditional_value_effects, generate_crossbow_charging_sounds,
    generate_damage_immunity_effects, generate_optional_value_effect, generate_sound_event_refs,
    generate_targeted_entity_effects, generate_targeted_value_effects,
};
use generation::generate_enchantment_effects;
use nbt::{NbtValueHint, generate_nbt_compound};
use parsing::{
    damage_type_ref_token, identifier_token, parse_enchantment_target, parse_entity_effect_json,
    parse_requirements_json, slot_to_tokens,
};

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
