//! Build script for generating vanilla loot table definitions.

use std::{fs, path::Path};

use heck::{ToShoutySnakeCase, ToSnakeCase};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use rustc_hash::FxHashMap;
use serde::Deserialize;

// ============================================================================
// Untagged enums for flexible JSON parsing
// ============================================================================

/// A number provider can be a constant number or an object with type.
#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
enum NumberProviderJson {
    Constant(f32),
    Object {
        #[serde(rename = "type")]
        provider_type: String,
        #[serde(default)]
        value: Option<f32>,
        #[serde(default)]
        min: Option<f32>,
        #[serde(default)]
        max: Option<f32>,
        #[serde(default)]
        n: Option<f32>, // Can be float in JSON, convert to i32 later
        #[serde(default)]
        p: Option<f32>,
    },
}

impl Default for NumberProviderJson {
    fn default() -> Self {
        Self::Constant(1.0)
    }
}

/// Enchantment options can be a tag string or list of enchantment IDs.
#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
enum EnchantmentOptionsJson {
    Tag(String),
    List(Vec<String>),
}

/// Loot table value can be a string reference or inline loot table.
#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
enum LootTableValueJson {
    Reference(String),
    Inline(Box<InlineLootTableJson>),
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct InlineLootTableJson {
    #[serde(default)]
    pools: Vec<LootPoolJson>,
}

/// Enchanted chance can be a constant or linear formula.
#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
enum EnchantedChanceJson {
    Constant(f32),
    Formula {
        #[serde(rename = "type")]
        formula_type: String,
        #[serde(default)]
        value: Option<f32>,
        #[serde(default)]
        base: Option<f32>,
        #[serde(default)]
        per_level_above_first: Option<f32>,
    },
}

/// Limit count can be an integer or object with min/max.
#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
enum LimitJson {
    Integer(i32),
    Object {
        #[serde(default)]
        min: Option<f32>,
        #[serde(default)]
        max: Option<f32>,
    },
}

/// Block state property value can be string or object with min/max.
#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
enum PropertyValueJson {
    Exact(String),
    Range {
        min: Option<String>,
        max: Option<String>,
    },
}

/// Stew effect entry.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct StewEffectJson {
    #[serde(rename = "type")]
    effect_type: String,
    #[serde(default)]
    duration: NumberProviderJson,
}

// ============================================================================
// Main JSON structures
// ============================================================================

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct LootTableJson {
    #[serde(rename = "type")]
    loot_type: Option<String>,
    #[serde(default)]
    pools: Vec<LootPoolJson>,
    #[serde(default)]
    functions: Vec<LootFunctionJson>,
    #[serde(default)]
    random_sequence: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct LootPoolJson {
    #[serde(default)]
    rolls: NumberProviderJson,
    #[serde(default)]
    bonus_rolls: f32,
    #[serde(default)]
    entries: Vec<LootEntryJson>,
    #[serde(default)]
    conditions: Vec<LootConditionJson>,
    #[serde(default)]
    functions: Vec<LootFunctionJson>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct LootEntryJson {
    #[serde(rename = "type")]
    entry_type: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    value: Option<LootTableValueJson>,
    #[serde(default = "default_weight")]
    weight: i32,
    #[serde(default)]
    quality: i32,
    #[serde(default)]
    expand: bool,
    #[serde(default)]
    conditions: Vec<LootConditionJson>,
    #[serde(default)]
    functions: Vec<LootFunctionJson>,
    #[serde(default)]
    children: Vec<LootEntryJson>,
}

fn default_weight() -> i32 {
    1
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct LootConditionJson {
    condition: String,
    // block_state_property
    #[serde(default)]
    block: Option<String>,
    #[serde(default)]
    properties: Option<FxHashMap<String, PropertyValueJson>>,
    // match_tool / entity_properties predicate
    #[serde(default)]
    predicate: Option<PredicateJson>,
    // table_bonus / random_chance_with_enchanted_bonus
    #[serde(default)]
    enchantment: Option<String>,
    #[serde(default)]
    chances: Option<Vec<f32>>,
    // inverted
    #[serde(default)]
    term: Option<Box<LootConditionJson>>,
    // any_of / all_of
    #[serde(default)]
    terms: Option<Vec<LootConditionJson>>,
    // random_chance
    #[serde(default)]
    chance: Option<f32>,
    // random_chance_with_enchanted_bonus
    #[serde(default)]
    unenchanted_chance: Option<f32>,
    #[serde(default)]
    enchanted_chance: Option<EnchantedChanceJson>,
    // entity_properties / damage_source_properties
    #[serde(default)]
    entity: Option<String>,
    // location_check
    #[serde(default, rename = "offsetX")]
    offset_x: Option<i32>,
    #[serde(default, rename = "offsetY")]
    offset_y: Option<i32>,
    #[serde(default, rename = "offsetZ")]
    offset_z: Option<i32>,
}

/// Predicate can be a tool predicate (match_tool), location predicate (location_check),
/// entity predicate (entity_properties), or damage source predicate. We parse these specifically.
#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
enum PredicateJson {
    Tool(ToolPredicateJson),
    Location(LocationPredicateJson),
    DamageSource(DamageSourcePredicateJson),
    Entity(EntityPredicateJson),
}

/// Damage source predicate for damage_source_properties condition.
#[derive(Deserialize, Debug, Clone)]
struct DamageSourcePredicateJson {
    #[serde(default)]
    tags: Option<Vec<DamageTagPredicateJson>>,
    #[serde(default)]
    source_entity: Option<EntityPredicateJson>,
    #[serde(default)]
    direct_entity: Option<EntityPredicateJson>,
    #[serde(default)]
    is_direct: Option<bool>,
}

/// A tag check for damage source.
#[derive(Deserialize, Debug, Clone)]
struct DamageTagPredicateJson {
    id: String,
    #[serde(default = "default_true")]
    expected: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct LocationPredicateJson {
    #[serde(default)]
    block: Option<BlockPredicateJson>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct BlockPredicateJson {
    #[serde(default)]
    blocks: Option<String>,
    #[serde(default)]
    state: Option<FxHashMap<String, String>>,
}

/// Entity predicate - can have many fields
#[derive(Deserialize, Debug, Clone)]
struct EntityPredicateJson {
    #[serde(rename = "type", default)]
    entity_type: Option<String>,
    #[serde(default)]
    flags: Option<EntityFlagsJson>,
    #[serde(default)]
    equipment: Option<EntityEquipmentJson>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct EntityFlagsJson {
    #[serde(default)]
    is_on_fire: Option<bool>,
    #[serde(default)]
    is_sneaking: Option<bool>,
    #[serde(default)]
    is_sprinting: Option<bool>,
    #[serde(default)]
    is_swimming: Option<bool>,
    #[serde(default)]
    is_baby: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct EntityEquipmentJson {
    #[serde(default)]
    mainhand: Option<EquipmentSlotJson>,
    #[serde(default)]
    offhand: Option<EquipmentSlotJson>,
    #[serde(default)]
    head: Option<EquipmentSlotJson>,
    #[serde(default)]
    chest: Option<EquipmentSlotJson>,
    #[serde(default)]
    legs: Option<EquipmentSlotJson>,
    #[serde(default)]
    feet: Option<EquipmentSlotJson>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct EquipmentSlotJson {
    #[serde(default)]
    items: Option<String>,
    #[serde(default)]
    predicates: Option<ToolPredicatesJson>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ToolPredicateJson {
    #[serde(default)]
    items: Option<String>,
    #[serde(default)]
    predicates: Option<ToolPredicatesJson>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ToolPredicatesJson {
    #[serde(rename = "minecraft:enchantments", default)]
    enchantments: Option<Vec<EnchantmentPredicateJson>>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct EnchantmentPredicateJson {
    #[serde(default)]
    enchantments: Option<String>,
    #[serde(default)]
    levels: Option<LevelRangeJson>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct LevelRangeJson {
    #[serde(default)]
    min: Option<i32>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct LootFunctionJson {
    function: String,
    #[serde(default)]
    count: Option<NumberProviderJson>,
    #[serde(default)]
    add: bool,
    // apply_bonus
    #[serde(default)]
    enchantment: Option<String>,
    #[serde(default)]
    formula: Option<String>,
    #[serde(default)]
    parameters: Option<BonusParametersJson>,
    // limit_count / enchanted_count_increase limit
    #[serde(default)]
    limit: Option<LimitJson>,
    // set_damage
    #[serde(default)]
    damage: Option<NumberProviderJson>,
    // enchant_randomly / enchant_with_levels / set_instrument
    #[serde(default)]
    options: Option<EnchantmentOptionsJson>,
    // enchant_with_levels
    #[serde(default)]
    levels: Option<NumberProviderJson>,
    // copy_components
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    include: Option<Vec<String>>,
    // copy_state
    #[serde(default)]
    block: Option<String>,
    // copy_state properties
    #[serde(default)]
    properties: Option<Vec<String>>,
    // set_components (keep as raw value since it's complex NBT)
    #[serde(default)]
    components: Option<serde_json::Value>,
    // exploration_map
    #[serde(default)]
    destination: Option<String>,
    #[serde(default)]
    decoration: Option<String>,
    #[serde(default)]
    zoom: Option<i32>,
    #[serde(default)]
    skip_existing_chunks: Option<bool>,
    // set_name (keep as raw value for text component)
    #[serde(default)]
    name: Option<serde_json::Value>,
    #[serde(default)]
    target: Option<String>,
    // set_ominous_bottle_amplifier
    #[serde(default)]
    amplifier: Option<NumberProviderJson>,
    // set_potion
    #[serde(default)]
    id: Option<String>,
    // set_stew_effect
    #[serde(default)]
    effects: Option<Vec<StewEffectJson>>,
    // set_enchantments
    #[serde(default)]
    enchantments: Option<FxHashMap<String, NumberProviderJson>>,
    // conditions for conditional functions
    #[serde(default)]
    conditions: Option<Vec<LootConditionJson>>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct BonusParametersJson {
    #[serde(rename = "bonusMultiplier", default)]
    bonus_multiplier: Option<i32>,
    #[serde(default)]
    extra: Option<i32>,
    #[serde(default)]
    probability: Option<f32>,
}

// ============================================================================
// Code generation functions
// ============================================================================

fn generate_number_provider(value: &NumberProviderJson) -> TokenStream {
    match value {
        NumberProviderJson::Constant(v) => {
            quote! { NumberProvider::Constant(#v) }
        }
        NumberProviderJson::Object {
            provider_type,
            value,
            min,
            max,
            n,
            p,
        } => match provider_type.as_str() {
            "minecraft:uniform" => {
                let min = min.unwrap_or(0.0);
                let max = max.unwrap_or(1.0);
                quote! { NumberProvider::Uniform { min: #min, max: #max } }
            }
            "minecraft:binomial" => {
                let n = n.unwrap_or(1.0) as i32;
                let p = p.unwrap_or(0.5);
                quote! { NumberProvider::Binomial { n: #n, p: #p } }
            }
            _ => {
                let v = value.unwrap_or(1.0);
                quote! { NumberProvider::Constant(#v) }
            }
        },
    }
}

/// Generate the LootContextEntity enum variant at build time.
fn generate_loot_context_entity(entity: &str) -> TokenStream {
    match entity {
        "this" => quote! { LootContextEntity::This },
        "killer" | "attacker" => quote! { LootContextEntity::Killer },
        "direct_killer" | "direct_attacker" => quote! { LootContextEntity::DirectKiller },
        "killer_player" | "last_damage_player" => quote! { LootContextEntity::KillerPlayer },
        "interacting_entity" => quote! { LootContextEntity::Interacting },
        _ => quote! { LootContextEntity::This },
    }
}

/// Generate the EquipmentSlotGroup enum variant at build time.
#[allow(dead_code)]
fn generate_equipment_slot_group(slot: &str) -> TokenStream {
    match slot {
        "any" => quote! { EquipmentSlotGroup::Any },
        "mainhand" | "main_hand" => quote! { EquipmentSlotGroup::MainHand },
        "offhand" | "off_hand" => quote! { EquipmentSlotGroup::OffHand },
        "hand" => quote! { EquipmentSlotGroup::Hand },
        "head" => quote! { EquipmentSlotGroup::Head },
        "chest" => quote! { EquipmentSlotGroup::Chest },
        "legs" => quote! { EquipmentSlotGroup::Legs },
        "feet" => quote! { EquipmentSlotGroup::Feet },
        "armor" => quote! { EquipmentSlotGroup::Armor },
        "body" => quote! { EquipmentSlotGroup::Body },
        _ => quote! { EquipmentSlotGroup::Any },
    }
}

/// Generate the DyeColor enum variant at build time.
#[allow(dead_code)]
fn generate_dye_color(color: &str) -> TokenStream {
    match color {
        "white" => quote! { DyeColor::White },
        "orange" => quote! { DyeColor::Orange },
        "magenta" => quote! { DyeColor::Magenta },
        "light_blue" => quote! { DyeColor::LightBlue },
        "yellow" => quote! { DyeColor::Yellow },
        "lime" => quote! { DyeColor::Lime },
        "pink" => quote! { DyeColor::Pink },
        "gray" => quote! { DyeColor::Gray },
        "light_gray" => quote! { DyeColor::LightGray },
        "cyan" => quote! { DyeColor::Cyan },
        "purple" => quote! { DyeColor::Purple },
        "blue" => quote! { DyeColor::Blue },
        "brown" => quote! { DyeColor::Brown },
        "green" => quote! { DyeColor::Green },
        "red" => quote! { DyeColor::Red },
        "black" => quote! { DyeColor::Black },
        _ => quote! { DyeColor::White },
    }
}

/// Generate the LootType enum variant at build time.
fn generate_loot_type(loot_type: &str) -> TokenStream {
    match loot_type {
        "minecraft:block" => quote! { LootType::Block },
        "minecraft:entity" => quote! { LootType::Entity },
        "minecraft:chest" => quote! { LootType::Chest },
        "minecraft:fishing" => quote! { LootType::Fishing },
        "minecraft:gift" => quote! { LootType::Gift },
        "minecraft:archaeology" => quote! { LootType::Archaeology },
        "minecraft:vault" => quote! { LootType::Vault },
        "minecraft:shearing" => quote! { LootType::Shearing },
        "minecraft:equipment" => quote! { LootType::Equipment },
        "minecraft:selector" => quote! { LootType::Selector },
        "minecraft:entity_interact" => quote! { LootType::EntityInteract },
        "minecraft:block_interact" => quote! { LootType::BlockInteract },
        "minecraft:barter" => quote! { LootType::Barter },
        _ => quote! { LootType::Block }, // Default to Block
    }
}

fn generate_tool_predicate(predicate: &Option<PredicateJson>) -> TokenStream {
    let Some(pred) = predicate else {
        return quote! { ToolPredicate::Any };
    };

    // Only handle tool predicates; location/entity/damage_source predicates return Any
    let pred = match pred {
        PredicateJson::Tool(p) => p,
        PredicateJson::Location(_) => return quote! { ToolPredicate::Any },
        PredicateJson::DamageSource(_) => return quote! { ToolPredicate::Any },
        PredicateJson::Entity(_) => return quote! { ToolPredicate::Any },
    };

    // Check for items field (can be a string or tag reference)
    if let Some(item_str) = &pred.items {
        if item_str.starts_with('#') {
            // Tag reference like "#minecraft:pickaxes"
            let tag = item_str
                .strip_prefix("#minecraft:")
                .unwrap_or(item_str.strip_prefix('#').unwrap_or(item_str));
            return quote! { ToolPredicate::Tag(Identifier::vanilla_static(#tag)) };
        } else {
            let item = item_str.strip_prefix("minecraft:").unwrap_or(item_str);
            return quote! { ToolPredicate::Item(Identifier::vanilla_static(#item)) };
        }
    }

    // Check for enchantment predicates
    if let Some(predicates) = &pred.predicates
        && let Some(enchants) = &predicates.enchantments
        && let Some(first) = enchants.first()
        && let Some(enchant_name) = &first.enchantments
    {
        let enchant_name = enchant_name
            .strip_prefix("minecraft:")
            .unwrap_or(enchant_name);
        let min_level = first.levels.as_ref().and_then(|l| l.min).unwrap_or(1);

        return quote! {
            ToolPredicate::HasEnchantment {
                enchantment: Identifier::vanilla_static(#enchant_name),
                min_level: #min_level,
            }
        };
    }

    quote! { ToolPredicate::Any }
}

fn generate_enchantment_options(options: &Option<EnchantmentOptionsJson>) -> TokenStream {
    match options {
        Some(EnchantmentOptionsJson::Tag(s)) => {
            let tag = s
                .strip_prefix("#minecraft:")
                .unwrap_or(s.strip_prefix("minecraft:").unwrap_or(s));
            quote! { EnchantmentOptions::Tag(Identifier::vanilla_static(#tag)) }
        }
        Some(EnchantmentOptionsJson::List(arr)) => {
            let enchants: Vec<TokenStream> = arr
                .iter()
                .map(|s| {
                    let s = s.strip_prefix("minecraft:").unwrap_or(s);
                    quote! { Identifier::vanilla_static(#s) }
                })
                .collect();
            quote! { EnchantmentOptions::List(&[#(#enchants),*]) }
        }
        None => {
            quote! { EnchantmentOptions::Tag(Identifier::vanilla_static("on_random_loot")) }
        }
    }
}

fn generate_entity_flags(flags: &Option<EntityFlagsJson>) -> TokenStream {
    match flags {
        Some(f) => {
            let is_on_fire = match f.is_on_fire {
                Some(v) => quote! { Some(#v) },
                None => quote! { None },
            };
            let is_sneaking = match f.is_sneaking {
                Some(v) => quote! { Some(#v) },
                None => quote! { None },
            };
            let is_sprinting = match f.is_sprinting {
                Some(v) => quote! { Some(#v) },
                None => quote! { None },
            };
            let is_swimming = match f.is_swimming {
                Some(v) => quote! { Some(#v) },
                None => quote! { None },
            };
            let is_baby = match f.is_baby {
                Some(v) => quote! { Some(#v) },
                None => quote! { None },
            };
            quote! {
                Some(EntityFlags {
                    is_on_fire: #is_on_fire,
                    is_sneaking: #is_sneaking,
                    is_sprinting: #is_sprinting,
                    is_swimming: #is_swimming,
                    is_baby: #is_baby,
                })
            }
        }
        None => quote! { None },
    }
}

fn generate_equipment_slot_predicate(slot: &Option<EquipmentSlotJson>) -> TokenStream {
    match slot {
        Some(s) => {
            if let Some(items) = &s.items {
                if items.starts_with('#') {
                    let tag = items
                        .strip_prefix("#minecraft:")
                        .unwrap_or(items.strip_prefix('#').unwrap_or(items));
                    return quote! { Some(ToolPredicate::Tag(Identifier::vanilla_static(#tag))) };
                } else {
                    let item = items.strip_prefix("minecraft:").unwrap_or(items);
                    return quote! { Some(ToolPredicate::Item(Identifier::vanilla_static(#item))) };
                }
            }

            if let Some(predicates) = &s.predicates
                && let Some(enchants) = &predicates.enchantments
                && let Some(first) = enchants.first()
                && let Some(enchant_name) = &first.enchantments
            {
                let enchant_name = enchant_name
                    .strip_prefix("minecraft:")
                    .unwrap_or(enchant_name);
                let min_level = first.levels.as_ref().and_then(|l| l.min).unwrap_or(1);
                return quote! {
                    Some(ToolPredicate::HasEnchantment {
                        enchantment: Identifier::vanilla_static(#enchant_name),
                        min_level: #min_level,
                    })
                };
            }

            quote! { Some(ToolPredicate::Any) }
        }
        None => quote! { None },
    }
}

fn generate_entity_equipment(equipment: &Option<EntityEquipmentJson>) -> TokenStream {
    match equipment {
        Some(e) => {
            let mainhand = generate_equipment_slot_predicate(&e.mainhand);
            let offhand = generate_equipment_slot_predicate(&e.offhand);
            let head = generate_equipment_slot_predicate(&e.head);
            let chest = generate_equipment_slot_predicate(&e.chest);
            let legs = generate_equipment_slot_predicate(&e.legs);
            let feet = generate_equipment_slot_predicate(&e.feet);

            quote! {
                Some(EntityEquipment {
                    mainhand: #mainhand,
                    offhand: #offhand,
                    head: #head,
                    chest: #chest,
                    legs: #legs,
                    feet: #feet,
                })
            }
        }
        None => quote! { None },
    }
}

fn generate_entity_predicate(predicate: &EntityPredicateJson) -> TokenStream {
    let entity_type = match &predicate.entity_type {
        Some(t) => {
            let t = t.strip_prefix("minecraft:").unwrap_or(t);
            quote! { Some(Identifier::vanilla_static(#t)) }
        }
        None => quote! { None },
    };

    let flags = generate_entity_flags(&predicate.flags);
    let equipment = generate_entity_equipment(&predicate.equipment);

    quote! {
        EntityPredicate {
            entity_type: #entity_type,
            flags: #flags,
            equipment: #equipment,
        }
    }
}

fn generate_damage_source_predicate(predicate: &DamageSourcePredicateJson) -> TokenStream {
    let tags: Vec<TokenStream> = predicate
        .tags
        .as_ref()
        .map(|t| {
            t.iter()
                .map(|tag| {
                    let id = tag.id.strip_prefix("minecraft:").unwrap_or(&tag.id);
                    let expected = tag.expected;
                    quote! {
                        DamageTagPredicate {
                            id: Identifier::vanilla_static(#id),
                            expected: #expected,
                        }
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let source_entity = match &predicate.source_entity {
        Some(e) => {
            let pred = generate_entity_predicate(e);
            quote! { Some(#pred) }
        }
        None => quote! { None },
    };

    let direct_entity = match &predicate.direct_entity {
        Some(e) => {
            let pred = generate_entity_predicate(e);
            quote! { Some(#pred) }
        }
        None => quote! { None },
    };

    let is_direct = match predicate.is_direct {
        Some(v) => quote! { Some(#v) },
        None => quote! { None },
    };

    quote! {
        DamageSourcePredicate {
            tags: &[#(#tags),*],
            source_entity: #source_entity,
            direct_entity: #direct_entity,
            is_direct: #is_direct,
        }
    }
}

fn generate_block_predicate(predicate: &BlockPredicateJson) -> TokenStream {
    let blocks = match &predicate.blocks {
        Some(b) => {
            let b = b.strip_prefix("minecraft:").unwrap_or(b);
            quote! { Some(Identifier::vanilla_static(#b)) }
        }
        None => quote! { None },
    };

    let state: Vec<TokenStream> = predicate
        .state
        .as_ref()
        .map(|props| {
            props
                .iter()
                .map(|(name, value)| {
                    quote! { (#name, #value) }
                })
                .collect()
        })
        .unwrap_or_default();

    quote! {
        BlockPredicate {
            blocks: #blocks,
            state: &[#(#state),*],
        }
    }
}

fn generate_location_predicate(predicate: &LocationPredicateJson) -> TokenStream {
    let block = match &predicate.block {
        Some(b) => {
            let block_pred = generate_block_predicate(b);
            quote! { Some(#block_pred) }
        }
        None => quote! { None },
    };

    quote! {
        LocationPredicate {
            block: #block,
        }
    }
}

fn generate_condition(condition: &LootConditionJson) -> TokenStream {
    match condition.condition.as_str() {
        "minecraft:survives_explosion" => {
            quote! { LootCondition::SurvivesExplosion }
        }
        "minecraft:block_state_property" => {
            let block = condition.block.as_deref().unwrap_or("minecraft:air");
            let block = block.strip_prefix("minecraft:").unwrap_or(block);

            let properties: Vec<TokenStream> = condition
                .properties
                .as_ref()
                .map(|props| {
                    props
                        .iter()
                        .map(|(name, value)| {
                            let value_str = match value {
                                PropertyValueJson::Exact(s) => s.clone(),
                                PropertyValueJson::Range { min, max } => {
                                    // For range values, use a string representation
                                    format!(
                                        "{}..{}",
                                        min.as_deref().unwrap_or(""),
                                        max.as_deref().unwrap_or("")
                                    )
                                }
                            };
                            quote! { PropertyCheck { name: #name, value: #value_str } }
                        })
                        .collect()
                })
                .unwrap_or_default();

            quote! {
                LootCondition::BlockStateProperty {
                    block: Identifier::vanilla_static(#block),
                    properties: &[#(#properties),*],
                }
            }
        }
        "minecraft:match_tool" => {
            let predicate = generate_tool_predicate(&condition.predicate);
            quote! { LootCondition::MatchTool(#predicate) }
        }
        "minecraft:table_bonus" => {
            let enchantment = condition
                .enchantment
                .as_deref()
                .unwrap_or("minecraft:fortune");
            let enchantment = enchantment
                .strip_prefix("minecraft:")
                .unwrap_or(enchantment);

            let chances: Vec<TokenStream> = condition
                .chances
                .as_ref()
                .map(|c| c.iter().map(|v| quote! { #v }).collect())
                .unwrap_or_default();

            quote! {
                LootCondition::TableBonus {
                    enchantment: Identifier::vanilla_static(#enchantment),
                    chances: &[#(#chances),*],
                }
            }
        }
        "minecraft:inverted" => {
            if let Some(term) = &condition.term {
                let inner = generate_condition(term);
                quote! { LootCondition::Inverted(&{ #inner }) }
            } else {
                quote! { LootCondition::Inverted(&LootCondition::RandomChance(1.0)) }
            }
        }
        "minecraft:any_of" => {
            let terms: Vec<TokenStream> = condition
                .terms
                .as_ref()
                .map(|t| t.iter().map(generate_condition).collect())
                .unwrap_or_default();

            quote! { LootCondition::AnyOf(&[#(#terms),*]) }
        }
        "minecraft:all_of" => {
            let terms: Vec<TokenStream> = condition
                .terms
                .as_ref()
                .map(|t| t.iter().map(generate_condition).collect())
                .unwrap_or_default();

            quote! { LootCondition::AllOf(&[#(#terms),*]) }
        }
        "minecraft:random_chance" => {
            let chance = condition.chance.unwrap_or(0.5);
            quote! { LootCondition::RandomChance(#chance) }
        }
        "minecraft:random_chance_with_enchanted_bonus" => {
            let enchantment = condition
                .enchantment
                .as_deref()
                .unwrap_or("minecraft:looting");
            let enchantment = enchantment
                .strip_prefix("minecraft:")
                .unwrap_or(enchantment);

            let unenchanted_chance = condition.unenchanted_chance.unwrap_or(0.0);

            let enchanted_chance = match &condition.enchanted_chance {
                Some(EnchantedChanceJson::Constant(v)) => {
                    quote! { EnchantedChance::Constant(#v) }
                }
                Some(EnchantedChanceJson::Formula {
                    formula_type,
                    value,
                    base,
                    per_level_above_first,
                }) => {
                    if formula_type == "minecraft:linear" {
                        let base = base.unwrap_or(0.0);
                        let per_level = per_level_above_first.unwrap_or(0.0);
                        quote! { EnchantedChance::Linear { base: #base, per_level_above_first: #per_level } }
                    } else {
                        let v = value.unwrap_or(0.0);
                        quote! { EnchantedChance::Constant(#v) }
                    }
                }
                None => quote! { EnchantedChance::Constant(0.0) },
            };

            quote! {
                LootCondition::RandomChanceWithEnchantedBonus {
                    enchantment: Identifier::vanilla_static(#enchantment),
                    unenchanted_chance: #unenchanted_chance,
                    enchanted_chance: #enchanted_chance,
                }
            }
        }
        "minecraft:killed_by_player" => {
            quote! { LootCondition::KilledByPlayer }
        }
        "minecraft:entity_properties" => {
            let entity = condition.entity.as_deref().unwrap_or("this");
            let entity_variant = generate_loot_context_entity(entity);

            let predicate = if let Some(pred) = &condition.predicate {
                match pred {
                    PredicateJson::Entity(e) => generate_entity_predicate(e),
                    _ => quote! {
                        EntityPredicate {
                            entity_type: None,
                            flags: None,
                            equipment: None,
                        }
                    },
                }
            } else {
                quote! {
                    EntityPredicate {
                        entity_type: None,
                        flags: None,
                        equipment: None,
                    }
                }
            };

            quote! {
                LootCondition::EntityProperties {
                    entity: #entity_variant,
                    predicate: #predicate,
                }
            }
        }
        "minecraft:damage_source_properties" => {
            let predicate = if let Some(pred) = &condition.predicate {
                match pred {
                    PredicateJson::DamageSource(ds) => generate_damage_source_predicate(ds),
                    _ => quote! {
                        DamageSourcePredicate {
                            tags: &[],
                            source_entity: None,
                            direct_entity: None,
                            is_direct: None,
                        }
                    },
                }
            } else {
                quote! {
                    DamageSourcePredicate {
                        tags: &[],
                        source_entity: None,
                        direct_entity: None,
                        is_direct: None,
                    }
                }
            };

            quote! {
                LootCondition::DamageSourceProperties {
                    predicate: #predicate,
                }
            }
        }
        "minecraft:location_check" => {
            let offset_x = condition.offset_x.unwrap_or(0);
            let offset_y = condition.offset_y.unwrap_or(0);
            let offset_z = condition.offset_z.unwrap_or(0);

            let predicate = if let Some(pred) = &condition.predicate {
                match pred {
                    PredicateJson::Location(l) => generate_location_predicate(l),
                    _ => quote! {
                        LocationPredicate {
                            block: None,
                        }
                    },
                }
            } else {
                quote! {
                    LocationPredicate {
                        block: None,
                    }
                }
            };

            quote! {
                LootCondition::LocationCheck {
                    offset_x: #offset_x,
                    offset_y: #offset_y,
                    offset_z: #offset_z,
                    predicate: #predicate,
                }
            }
        }
        other => {
            panic!("Unknown loot condition type: {}", other);
        }
    }
}

fn generate_function(function: &LootFunctionJson) -> TokenStream {
    let func_body = match function.function.as_str() {
        "minecraft:set_count" => {
            let count = function
                .count
                .as_ref()
                .map(generate_number_provider)
                .unwrap_or_else(|| quote! { NumberProvider::Constant(1.0) });
            let add = function.add;
            quote! { LootFunction::SetCount { count: #count, add: #add } }
        }
        "minecraft:explosion_decay" => {
            quote! { LootFunction::ExplosionDecay }
        }
        "minecraft:apply_bonus" => {
            let enchantment = function
                .enchantment
                .as_deref()
                .unwrap_or("minecraft:fortune");
            let enchantment = enchantment
                .strip_prefix("minecraft:")
                .unwrap_or(enchantment);

            let formula = match function.formula.as_deref() {
                Some("minecraft:ore_drops") => {
                    quote! { BonusFormula::OreDrops }
                }
                Some("minecraft:uniform_bonus_count") => {
                    let multiplier = function
                        .parameters
                        .as_ref()
                        .and_then(|p| p.bonus_multiplier)
                        .unwrap_or(1);
                    quote! { BonusFormula::UniformBonusCount { bonus_multiplier: #multiplier } }
                }
                Some("minecraft:binomial_with_bonus_count") => {
                    let extra = function
                        .parameters
                        .as_ref()
                        .and_then(|p| p.extra)
                        .unwrap_or(0);
                    let probability = function
                        .parameters
                        .as_ref()
                        .and_then(|p| p.probability)
                        .unwrap_or(0.5);
                    quote! { BonusFormula::BinomialWithBonusCount { extra: #extra, probability: #probability } }
                }
                _ => {
                    quote! { BonusFormula::OreDrops }
                }
            };

            quote! {
                LootFunction::ApplyBonus {
                    enchantment: Identifier::vanilla_static(#enchantment),
                    formula: #formula,
                }
            }
        }
        "minecraft:enchanted_count_increase" => {
            let enchantment = function
                .enchantment
                .as_deref()
                .unwrap_or("minecraft:looting");
            let enchantment = enchantment
                .strip_prefix("minecraft:")
                .unwrap_or(enchantment);

            let count = function
                .count
                .as_ref()
                .map(generate_number_provider)
                .unwrap_or_else(|| quote! { NumberProvider::Uniform { min: 0.0, max: 1.0 } });

            let limit = match &function.limit {
                Some(LimitJson::Integer(v)) => *v,
                Some(LimitJson::Object { max, .. }) => max.map(|v| v as i32).unwrap_or(0),
                None => 0,
            };

            quote! {
                LootFunction::EnchantedCountIncrease {
                    enchantment: Identifier::vanilla_static(#enchantment),
                    count: #count,
                    limit: #limit,
                }
            }
        }
        "minecraft:limit_count" => {
            let (min, max) = match &function.limit {
                Some(LimitJson::Integer(v)) => (Some(*v), Some(*v)),
                Some(LimitJson::Object { min, max }) => {
                    (min.map(|v| v as i32), max.map(|v| v as i32))
                }
                None => (None, None),
            };

            let min_tokens = match min {
                Some(v) => quote! { Some(#v) },
                None => quote! { None },
            };
            let max_tokens = match max {
                Some(v) => quote! { Some(#v) },
                None => quote! { None },
            };

            quote! { LootFunction::LimitCount { min: #min_tokens, max: #max_tokens } }
        }
        "minecraft:set_damage" => {
            let damage = function
                .damage
                .as_ref()
                .map(generate_number_provider)
                .unwrap_or_else(|| quote! { NumberProvider::Constant(1.0) });
            let add = function.add;
            quote! { LootFunction::SetDamage { damage: #damage, add: #add } }
        }
        "minecraft:enchant_randomly" => {
            let options = generate_enchantment_options(&function.options);
            quote! { LootFunction::EnchantRandomly { options: #options } }
        }
        "minecraft:enchant_with_levels" => {
            let levels = function
                .levels
                .as_ref()
                .map(generate_number_provider)
                .unwrap_or_else(|| quote! { NumberProvider::Constant(30.0) });
            let options = generate_enchantment_options(&function.options);
            quote! {
                LootFunction::EnchantWithLevels {
                    levels: #levels,
                    options: #options,
                }
            }
        }
        "minecraft:copy_components" => {
            let source = match function.source.as_deref() {
                Some("block_entity") => quote! { CopySource::BlockEntity },
                Some("this") => quote! { CopySource::This },
                Some("attacker") => quote! { CopySource::Attacker },
                Some("direct_attacker") => quote! { CopySource::DirectAttacker },
                _ => quote! { CopySource::BlockEntity },
            };

            let include: Vec<TokenStream> = function
                .include
                .as_ref()
                .map(|inc| {
                    inc.iter()
                        .map(|s| {
                            let s = s.strip_prefix("minecraft:").unwrap_or(s);
                            quote! { Identifier::vanilla_static(#s) }
                        })
                        .collect()
                })
                .unwrap_or_default();

            quote! {
                LootFunction::CopyComponents {
                    source: #source,
                    include: &[#(#include),*],
                }
            }
        }
        "minecraft:copy_state" => {
            let block = function.block.as_deref().unwrap_or("minecraft:air");
            let block = block.strip_prefix("minecraft:").unwrap_or(block);

            let properties: Vec<TokenStream> = function
                .properties
                .as_ref()
                .map(|props| props.iter().map(|p| quote! { #p }).collect())
                .unwrap_or_default();

            quote! {
                LootFunction::CopyState {
                    block: Identifier::vanilla_static(#block),
                    properties: &[#(#properties),*],
                }
            }
        }
        "minecraft:set_components" => {
            let components_str = function
                .components
                .as_ref()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "{}".to_string());
            quote! { LootFunction::SetComponents { components: #components_str } }
        }
        "minecraft:furnace_smelt" => {
            quote! { LootFunction::FurnaceSmelt }
        }
        "minecraft:exploration_map" => {
            let destination = function
                .destination
                .as_deref()
                .unwrap_or("minecraft:buried_treasure");
            let destination = destination
                .strip_prefix("minecraft:")
                .unwrap_or(destination);

            let decoration = function.decoration.as_deref().unwrap_or("minecraft:red_x");
            let decoration = decoration.strip_prefix("minecraft:").unwrap_or(decoration);

            let zoom = function.zoom.unwrap_or(2);
            let skip_existing_chunks = function.skip_existing_chunks.unwrap_or(true);

            quote! {
                LootFunction::ExplorationMap {
                    destination: Identifier::vanilla_static(#destination),
                    decoration: Identifier::vanilla_static(#decoration),
                    zoom: #zoom,
                    skip_existing_chunks: #skip_existing_chunks,
                }
            }
        }
        "minecraft:set_name" => {
            let name_str = function
                .name
                .as_ref()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "\"\"".to_string());

            let target = match function.target.as_deref() {
                Some("custom_name") => quote! { NameTarget::CustomName },
                Some("item_name") => quote! { NameTarget::ItemName },
                _ => quote! { NameTarget::CustomName },
            };

            quote! {
                LootFunction::SetName {
                    name: #name_str,
                    target: #target,
                }
            }
        }
        "minecraft:set_ominous_bottle_amplifier" => {
            let amplifier = function
                .amplifier
                .as_ref()
                .map(generate_number_provider)
                .unwrap_or_else(|| quote! { NumberProvider::Constant(0.0) });
            quote! { LootFunction::SetOminousBottleAmplifier { amplifier: #amplifier } }
        }
        "minecraft:set_potion" => {
            let id = function.id.as_deref().unwrap_or("minecraft:water");
            let id = id.strip_prefix("minecraft:").unwrap_or(id);
            quote! { LootFunction::SetPotion { id: Identifier::vanilla_static(#id) } }
        }
        "minecraft:set_stew_effect" => {
            let effects: Vec<TokenStream> = function
                .effects
                .as_ref()
                .map(|effs| {
                    effs.iter()
                        .map(|e| {
                            let effect_type = e
                                .effect_type
                                .strip_prefix("minecraft:")
                                .unwrap_or(&e.effect_type);
                            let duration = generate_number_provider(&e.duration);

                            quote! {
                                StewEffect {
                                    effect_type: Identifier::vanilla_static(#effect_type),
                                    duration: #duration,
                                }
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();

            quote! { LootFunction::SetStewEffect { effects: &[#(#effects),*] } }
        }
        "minecraft:set_instrument" => {
            let options = match &function.options {
                Some(EnchantmentOptionsJson::Tag(s)) => {
                    let s = s
                        .strip_prefix("#minecraft:")
                        .unwrap_or(s.strip_prefix("minecraft:").unwrap_or(s));
                    quote! { Identifier::vanilla_static(#s) }
                }
                _ => quote! { Identifier::vanilla_static("regular_goat_horns") },
            };
            quote! { LootFunction::SetInstrument { options: #options } }
        }
        "minecraft:set_enchantments" => {
            let enchantments: Vec<TokenStream> = function
                .enchantments
                .as_ref()
                .map(|enc| {
                    enc.iter()
                        .map(|(name, level)| {
                            let name = name.strip_prefix("minecraft:").unwrap_or(name);
                            let level = generate_number_provider(level);
                            quote! { (Identifier::vanilla_static(#name), #level) }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let add = function.add;
            quote! {
                LootFunction::SetEnchantments {
                    enchantments: &[#(#enchantments),*],
                    add: #add,
                }
            }
        }
        other => {
            panic!("Unknown loot function type: {}", other);
        }
    };

    // Wrap the function with conditions
    let conditions: Vec<TokenStream> = function
        .conditions
        .as_ref()
        .map(|conds| conds.iter().map(generate_condition).collect())
        .unwrap_or_default();

    quote! {
        ConditionalLootFunction {
            function: #func_body,
            conditions: &[#(#conditions),*],
        }
    }
}

fn generate_entry(entry: &LootEntryJson) -> TokenStream {
    let conditions: Vec<TokenStream> = entry.conditions.iter().map(generate_condition).collect();
    let functions: Vec<TokenStream> = entry.functions.iter().map(generate_function).collect();

    match entry.entry_type.as_str() {
        "minecraft:item" => {
            let name = entry.name.as_deref().unwrap_or("minecraft:air");
            let name = name.strip_prefix("minecraft:").unwrap_or(name);
            let weight = entry.weight;
            let quality = entry.quality;
            quote! {
                LootEntry::Item {
                    name: Identifier::vanilla_static(#name),
                    weight: #weight,
                    quality: #quality,
                    conditions: &[#(#conditions),*],
                    functions: &[#(#functions),*],
                }
            }
        }
        "minecraft:loot_table" => {
            let weight = entry.weight;
            let quality = entry.quality;

            // Check if it's a string reference or inline loot table
            if let Some(name) = entry.name.as_deref() {
                let name = name.strip_prefix("minecraft:").unwrap_or(name);
                quote! {
                    LootEntry::LootTableRef {
                        name: Identifier::vanilla_static(#name),
                        weight: #weight,
                        quality: #quality,
                        conditions: &[#(#conditions),*],
                        functions: &[#(#functions),*],
                    }
                }
            } else if let Some(value) = &entry.value {
                match value {
                    LootTableValueJson::Reference(s) => {
                        let name = s.strip_prefix("minecraft:").unwrap_or(s);
                        quote! {
                            LootEntry::LootTableRef {
                                name: Identifier::vanilla_static(#name),
                                weight: #weight,
                                quality: #quality,
                                conditions: &[#(#conditions),*],
                                functions: &[#(#functions),*],
                            }
                        }
                    }
                    LootTableValueJson::Inline(inline) => {
                        let inline_pools: Vec<TokenStream> =
                            inline.pools.iter().map(generate_pool).collect();

                        quote! {
                            LootEntry::InlineLootTable {
                                pools: &[#(#inline_pools),*],
                                weight: #weight,
                                quality: #quality,
                                conditions: &[#(#conditions),*],
                                functions: &[#(#functions),*],
                            }
                        }
                    }
                }
            } else {
                quote! {
                    LootEntry::LootTableRef {
                        name: Identifier::vanilla_static("empty"),
                        weight: #weight,
                        quality: #quality,
                        conditions: &[#(#conditions),*],
                        functions: &[#(#functions),*],
                    }
                }
            }
        }
        "minecraft:tag" => {
            let name = entry.name.as_deref().unwrap_or("minecraft:empty");
            let name = name.strip_prefix("minecraft:").unwrap_or(name);
            let expand = entry.expand;
            let weight = entry.weight;
            let quality = entry.quality;
            quote! {
                LootEntry::Tag {
                    name: Identifier::vanilla_static(#name),
                    expand: #expand,
                    weight: #weight,
                    quality: #quality,
                    conditions: &[#(#conditions),*],
                    functions: &[#(#functions),*],
                }
            }
        }
        "minecraft:alternatives" => {
            let children: Vec<TokenStream> = entry.children.iter().map(generate_entry).collect();
            quote! {
                LootEntry::Alternatives {
                    children: &[#(#children),*],
                    conditions: &[#(#conditions),*],
                }
            }
        }
        "minecraft:group" => {
            let children: Vec<TokenStream> = entry.children.iter().map(generate_entry).collect();
            quote! {
                LootEntry::Group {
                    children: &[#(#children),*],
                    conditions: &[#(#conditions),*],
                }
            }
        }
        "minecraft:sequence" => {
            let children: Vec<TokenStream> = entry.children.iter().map(generate_entry).collect();
            quote! {
                LootEntry::Sequence {
                    children: &[#(#children),*],
                    conditions: &[#(#conditions),*],
                }
            }
        }
        "minecraft:empty" => {
            let weight = entry.weight;
            quote! {
                LootEntry::Empty {
                    weight: #weight,
                    conditions: &[#(#conditions),*],
                }
            }
        }
        "minecraft:dynamic" => {
            let name = entry.name.as_deref().unwrap_or("contents");
            let name = name.strip_prefix("minecraft:").unwrap_or(name);
            quote! {
                LootEntry::Dynamic {
                    name: Identifier::vanilla_static(#name),
                    conditions: &[#(#conditions),*],
                }
            }
        }
        other => {
            panic!("Unknown loot entry type: {}", other);
        }
    }
}

fn generate_pool(pool: &LootPoolJson) -> TokenStream {
    let rolls = generate_number_provider(&pool.rolls);
    let bonus_rolls = pool.bonus_rolls;
    let entries: Vec<TokenStream> = pool.entries.iter().map(generate_entry).collect();
    let conditions: Vec<TokenStream> = pool.conditions.iter().map(generate_condition).collect();
    let functions: Vec<TokenStream> = pool.functions.iter().map(generate_function).collect();

    quote! {
        LootPool {
            rolls: #rolls,
            bonus_rolls: #bonus_rolls,
            entries: &[#(#entries),*],
            conditions: &[#(#conditions),*],
            functions: &[#(#functions),*],
        }
    }
}

struct LootTableData {
    /// Full key path like "blocks/acacia_button"
    key: String,
    /// Rust identifier like "BLOCKS_ACACIA_BUTTON"
    const_ident: Ident,
    /// The loot type as a TokenStream
    loot_type: TokenStream,
    /// Generated pools
    pools: Vec<TokenStream>,
    /// Table-level functions
    functions: Vec<TokenStream>,
    /// Random sequence identifier
    random_sequence: Option<String>,
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/loot_table/"
    );

    let loot_table_dir = "build_assets/builtin_datapacks/minecraft/data/minecraft/loot_table";
    let mut tables: Vec<LootTableData> = Vec::new();

    // Recursively read all loot table JSON files
    fn read_loot_tables(dir: &Path, base_dir: &Path, tables: &mut Vec<LootTableData>) {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                read_loot_tables(&path, base_dir, tables);
            } else if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let relative_path = path
                    .strip_prefix(base_dir)
                    .unwrap_or(&path)
                    .with_extension("");
                let key = relative_path
                    .to_str()
                    .unwrap_or("unknown")
                    .replace('\\', "/");

                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                let loot_table: LootTableJson = match serde_json::from_str(&content) {
                    Ok(t) => t,
                    Err(e) => {
                        panic!("Failed to parse loot table {}: {}", key, e);
                    }
                };

                // Generate const identifier from the key
                let const_name = key.replace('/', "_").to_shouty_snake_case();
                let const_ident = Ident::new(&const_name, Span::call_site());

                let pools: Vec<TokenStream> = loot_table.pools.iter().map(generate_pool).collect();
                let functions: Vec<TokenStream> =
                    loot_table.functions.iter().map(generate_function).collect();

                let random_sequence = loot_table
                    .random_sequence
                    .as_ref()
                    .map(|s| s.strip_prefix("minecraft:").unwrap_or(s).to_string());

                tables.push(LootTableData {
                    key,
                    const_ident,
                    loot_type: generate_loot_type(
                        loot_table.loot_type.as_deref().unwrap_or("minecraft:empty"),
                    ),
                    pools,
                    functions,
                    random_sequence,
                });
            }
        }
    }

    read_loot_tables(
        Path::new(loot_table_dir),
        Path::new(loot_table_dir),
        &mut tables,
    );

    let mut stream = TokenStream::new();

    // Imports
    stream.extend(quote! {
        use crate::loot_table::{
            BlockPredicate, BonusFormula, ConditionalLootFunction, CopySource, DamageSourcePredicate,
            DamageTagPredicate, DyeColor, EnchantedChance, EnchantmentOptions, EntityEquipment,
            EntityFlags, EntityPredicate, EquipmentSlotGroup, LocationPredicate, LootCondition,
            LootContextEntity, LootEntry, LootFunction, LootPool, LootTable, LootTableRef,
            LootTableRegistry, LootType, NameTarget, NumberProvider, PropertyCheck, StewEffect,
            ToolPredicate,
        };
        use steel_utils::Identifier;
    });

    // Generate static constants for each loot table
    for table in &tables {
        let const_ident = &table.const_ident;
        let key = &table.key;
        let loot_type = &table.loot_type;
        let pools = &table.pools;
        let functions = &table.functions;

        let random_sequence = match &table.random_sequence {
            Some(seq) => quote! { Some(Identifier::vanilla_static(#seq)) },
            None => quote! { None },
        };

        stream.extend(quote! {
            pub static #const_ident: &LootTable = &LootTable {
                key: Identifier::vanilla_static(#key),
                loot_type: #loot_type,
                pools: &[#(#pools),*],
                functions: &[#(#functions),*],
                random_sequence: #random_sequence,
            };
        });
    }

    // Generate registration function
    let register_calls: Vec<TokenStream> = tables
        .iter()
        .map(|t| {
            let const_ident = &t.const_ident;
            quote! { registry.register(#const_ident); }
        })
        .collect();

    stream.extend(quote! {
        pub fn register_loot_tables(registry: &mut LootTableRegistry) {
            #(#register_calls)*
        }
    });

    // Generate a struct with categorized access for convenience
    // Group tables by their top-level directory
    let mut categories: std::collections::BTreeMap<String, Vec<(&LootTableData, Ident)>> =
        std::collections::BTreeMap::new();

    for table in &tables {
        let category = table.key.split('/').next().unwrap_or("other").to_string();
        let field_name = table
            .key
            .split('/')
            .skip(1)
            .collect::<Vec<_>>()
            .join("_")
            .to_snake_case();
        let field_name = if field_name.is_empty() {
            table.key.to_snake_case()
        } else {
            field_name
        };
        let field_ident = Ident::new(&field_name, Span::call_site());
        categories
            .entry(category)
            .or_default()
            .push((table, field_ident));
    }

    // Generate category structs
    for (category, items) in &categories {
        let struct_name = Ident::new(
            &format!(
                "{}LootTables",
                category
                    .to_snake_case()
                    .replace('_', " ")
                    .split_whitespace()
                    .map(|s| {
                        let mut c = s.chars();
                        match c.next() {
                            None => String::new(),
                            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                        }
                    })
                    .collect::<String>()
            ),
            Span::call_site(),
        );

        let fields: Vec<TokenStream> = items
            .iter()
            .map(|(_, field_ident)| {
                quote! { pub #field_ident: LootTableRef, }
            })
            .collect();

        let inits: Vec<TokenStream> = items
            .iter()
            .map(|(table, field_ident)| {
                let const_ident = &table.const_ident;
                quote! { #field_ident: #const_ident, }
            })
            .collect();

        stream.extend(quote! {
            pub struct #struct_name {
                #(#fields)*
            }

            impl #struct_name {
                pub const fn new() -> Self {
                    Self {
                        #(#inits)*
                    }
                }
            }
        });
    }

    // Generate the main LOOT_TABLES struct
    let category_fields: Vec<TokenStream> = categories
        .keys()
        .map(|category| {
            let field_ident = Ident::new(&category.to_snake_case(), Span::call_site());
            let struct_name = Ident::new(
                &format!(
                    "{}LootTables",
                    category
                        .to_snake_case()
                        .replace('_', " ")
                        .split_whitespace()
                        .map(|s| {
                            let mut c = s.chars();
                            match c.next() {
                                None => String::new(),
                                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                            }
                        })
                        .collect::<String>()
                ),
                Span::call_site(),
            );
            quote! { pub #field_ident: #struct_name, }
        })
        .collect();

    let category_inits: Vec<TokenStream> = categories
        .keys()
        .map(|category| {
            let field_ident = Ident::new(&category.to_snake_case(), Span::call_site());
            let struct_name = Ident::new(
                &format!(
                    "{}LootTables",
                    category
                        .to_snake_case()
                        .replace('_', " ")
                        .split_whitespace()
                        .map(|s| {
                            let mut c = s.chars();
                            match c.next() {
                                None => String::new(),
                                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                            }
                        })
                        .collect::<String>()
                ),
                Span::call_site(),
            );
            quote! { #field_ident: #struct_name::new(), }
        })
        .collect();

    stream.extend(quote! {
        pub struct LootTables {
            #(#category_fields)*
        }

        impl LootTables {
            pub const fn new() -> Self {
                Self {
                    #(#category_inits)*
                }
            }
        }

        pub static LOOT_TABLES: LootTables = LootTables::new();
    });

    stream
}
