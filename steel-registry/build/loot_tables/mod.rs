//! Build script for generating vanilla loot table definitions.

use std::{fs, path::Path, str::FromStr};

use heck::{ToShoutySnakeCase, ToSnakeCase};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use rustc_hash::FxHashMap;
use serde::Deserialize;
use steel_utils::Identifier;

mod conditions;
mod entries;
mod functions;
mod values;

use conditions::generate_condition;
use entries::generate_pool;
use functions::generate_function;
use values::{
    generate_damage_source_predicate, generate_enchantment_options, generate_entity_predicate,
    generate_instrument_options, generate_location_predicate, generate_loot_context_entity,
    generate_loot_type, generate_number_provider, generate_tool_predicate,
};

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

const fn default_weight() -> i32 {
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

/// Predicate can be a tool predicate (`match_tool`), location predicate (`location_check`),
/// entity predicate (`entity_properties`), or damage source predicate. We parse these specifically.
#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
#[expect(clippy::large_enum_variant)]
enum PredicateJson {
    Tool(ToolPredicateJson),
    Location(LocationPredicateJson),
    DamageSource(DamageSourcePredicateJson),
    Entity(EntityPredicateJson),
}

/// Damage source predicate for `damage_source_properties` condition.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
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

const fn default_true() -> bool {
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
    #[serde(rename = "type", alias = "minecraft:entity_type", default)]
    entity_type: Option<String>,
    #[serde(alias = "minecraft:flags", default)]
    flags: Option<EntityFlagsJson>,
    #[serde(alias = "minecraft:equipment", default)]
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
    // furnace_smelt
    #[serde(default)]
    use_input_count: Option<bool>,
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

struct LootTableData {
    /// Full key path like "`blocks/acacia_button`"
    key: String,
    /// Rust identifier like "`BLOCKS_ACACIA_BUTTON`"
    const_ident: Ident,
    /// The loot type as a `TokenStream`
    loot_type: TokenStream,
    /// Generated pools
    pools: Vec<TokenStream>,
    /// Table-level functions
    functions: Vec<TokenStream>,
    /// Random sequence identifier
    random_sequence: Option<String>,
}

pub(crate) fn build() -> TokenStream {
    let loot_table_dir = "../steel-utils/build_assets/builtin_datapacks/minecraft/loot_table";
    println!("cargo:rerun-if-changed={loot_table_dir}");
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
                        panic!("Failed to parse loot table {key}: {e}");
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
            EntityFlags, EntityPredicate, EquipmentSlotGroup, InstrumentOptions, LocationPredicate,
            LootCondition, LootContextEntity, LootEntry, LootFunction, LootPool, LootTable,
            LootTableRef, LootTableRegistry, LootType, NameTarget, NumberProvider, PropertyCheck,
            StewEffect, ToolPredicate,
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

        let random_sequence = if let Some(seq) = &table.random_sequence {
            quote! { Some(Identifier::vanilla_static(#seq)) }
        } else {
            quote! { None }
        };

        stream.extend(quote! {
            pub static #const_ident: LootTable = LootTable {
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
            quote! { registry.register(&#const_ident); }
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
                quote! { #field_ident: &#const_ident, }
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
