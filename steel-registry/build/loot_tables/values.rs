use super::{
    AttributeModifierJson, BannerPatternJson, BlockPredicateJson, DamageSourcePredicateJson,
    EnchantmentOptionsJson, EntityEquipmentJson, EntityFlagsJson, EntityPredicateJson,
    EquipmentSlotJson, FromStr, Ident, Identifier, LocationPredicateJson, NumberProviderJson,
    NumberProviderRangeJson, PredicateJson, ScoreboardTargetJson, Span, ToShoutySnakeCase,
    TokenStream, ToolPredicateJson, ToolPredicatesJson, generate_option,
    generate_static_identifier_from_str, quote,
};
use crate::nbt::{generate_nbt_compound, parse_lenient_compound};

pub(super) fn number_provider_constant(value: &NumberProviderJson) -> Option<f32> {
    match value {
        NumberProviderJson::Constant(value) => Some(*value),
        _ => None,
    }
}

fn generate_uniform_number_provider(
    min: &NumberProviderJson,
    max: &NumberProviderJson,
) -> TokenStream {
    if let (Some(min), Some(max)) = (number_provider_constant(min), number_provider_constant(max)) {
        return quote! { NumberProvider::Uniform { min: #min, max: #max } };
    }

    let min = generate_number_provider(min);
    let max = generate_number_provider(max);
    quote! {
        NumberProvider::UniformProvider {
            min: &#min,
            max: &#max,
        }
    }
}

fn generate_scoreboard_target(target: Option<&ScoreboardTargetJson>) -> TokenStream {
    match target {
        Some(ScoreboardTargetJson::Name(name)) => match name.as_str() {
            "this" => quote! { ScoreboardTarget::This },
            "killer" => quote! { ScoreboardTarget::Killer },
            "direct_killer" => quote! { ScoreboardTarget::DirectKiller },
            "killer_player" => quote! { ScoreboardTarget::KillerPlayer },
            fixed => quote! { ScoreboardTarget::Fixed(#fixed) },
        },
        Some(ScoreboardTargetJson::Object { target_type, name }) => match target_type.as_str() {
            "minecraft:this" | "this" => quote! { ScoreboardTarget::This },
            "minecraft:killer" | "killer" => quote! { ScoreboardTarget::Killer },
            "minecraft:direct_killer" | "direct_killer" => {
                quote! { ScoreboardTarget::DirectKiller }
            }
            "minecraft:killer_player" | "killer_player" => {
                quote! { ScoreboardTarget::KillerPlayer }
            }
            "minecraft:fixed" | "fixed" => {
                let name = name
                    .as_deref()
                    .unwrap_or_else(|| panic!("fixed scoreboard target missing name"));
                quote! { ScoreboardTarget::Fixed(#name) }
            }
            other => panic!("Unknown scoreboard target type: {other}"),
        },
        None => quote! { ScoreboardTarget::This },
    }
}

pub(super) fn generate_number_provider_range(range: &NumberProviderRangeJson) -> TokenStream {
    match range {
        NumberProviderRangeJson::Exact(value) => quote! { NumberProviderRange::exact(#value) },
        NumberProviderRangeJson::Range { min, max } => {
            let min = generate_option(min, generate_number_provider);
            let max = generate_option(max, generate_number_provider);
            quote! {
                NumberProviderRange {
                    min: #min,
                    max: #max,
                }
            }
        }
    }
}

pub(super) fn generate_number_provider(value: &NumberProviderJson) -> TokenStream {
    match value {
        NumberProviderJson::Constant(v) => {
            quote! { NumberProvider::Constant(#v) }
        }
        NumberProviderJson::UniformRange(range) => {
            generate_uniform_number_provider(&range.min, &range.max)
        }
        NumberProviderJson::Object {
            provider_type,
            value,
            min,
            max,
            n,
            p,
            target,
            score,
            scale,
        } => match provider_type.as_str() {
            "minecraft:uniform" => {
                let default_min = NumberProviderJson::Constant(0.0);
                let default_max = NumberProviderJson::Constant(1.0);
                let min = min.as_deref().unwrap_or(&default_min);
                let max = max.as_deref().unwrap_or(&default_max);
                generate_uniform_number_provider(min, max)
            }
            "minecraft:binomial" => {
                let n = n.unwrap_or(1.0) as i32;
                let p = p.unwrap_or(0.5);
                quote! { NumberProvider::Binomial { n: #n, p: #p } }
            }
            "minecraft:score" => {
                let target = generate_scoreboard_target(target.as_ref());
                let score = score
                    .as_deref()
                    .unwrap_or_else(|| panic!("score number provider missing score"));
                let scale = scale.unwrap_or(1.0);
                quote! {
                    NumberProvider::Score {
                        target: #target,
                        score: #score,
                        scale: #scale,
                    }
                }
            }
            _ => {
                let v = value.unwrap_or(1.0);
                quote! { NumberProvider::Constant(#v) }
            }
        },
    }
}

/// Generate the LootContextEntity enum variant at build time.
pub(super) fn generate_loot_context_entity(entity: &str) -> TokenStream {
    match entity {
        "this" => quote! { LootContextEntity::This },
        "killer" | "attacker" => quote! { LootContextEntity::Killer },
        "direct_killer" | "direct_attacker" => quote! { LootContextEntity::DirectKiller },
        "killer_player" | "last_damage_player" => quote! { LootContextEntity::KillerPlayer },
        "interacting_entity" => quote! { LootContextEntity::Interacting },
        _ => quote! { LootContextEntity::This },
    }
}

/// Generate the `EquipmentSlotGroup` enum variant at build time.
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

fn generate_attribute_operation(operation: &str) -> TokenStream {
    match operation {
        "add_value" => quote! { AttributeOperation::AddValue },
        "add_multiplied_base" => quote! { AttributeOperation::AddMultipliedBase },
        "add_multiplied_total" => quote! { AttributeOperation::AddMultipliedTotal },
        other => panic!("Unknown attribute modifier operation: {other}"),
    }
}

pub(super) fn generate_attribute_modifier(modifier: &AttributeModifierJson) -> TokenStream {
    let attribute = generate_static_identifier_from_str(&modifier.attribute, "attribute modifier");
    let operation = generate_attribute_operation(&modifier.operation);
    let amount = generate_number_provider(&modifier.amount);
    let id = generate_static_identifier_from_str(&modifier.id, "attribute modifier");
    let slot = generate_equipment_slot_group(&modifier.slot);
    quote! {
        AttributeModifier {
            attribute: #attribute,
            operation: #operation,
            amount: #amount,
            id: #id,
            slot: #slot,
        }
    }
}

pub(super) fn generate_banner_pattern(pattern: &BannerPatternJson) -> TokenStream {
    let pattern_id = generate_static_identifier_from_str(&pattern.pattern, "banner pattern");
    let color = generate_dye_color(&pattern.color);
    quote! {
        BannerPattern {
            pattern: #pattern_id,
            color: #color,
        }
    }
}

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

pub(super) fn generate_firework_shape(shape: &str) -> TokenStream {
    match shape {
        "small_ball" => quote! { FireworkShape::SmallBall },
        "large_ball" => quote! { FireworkShape::LargeBall },
        "star" => quote! { FireworkShape::Star },
        "creeper" => quote! { FireworkShape::Creeper },
        "burst" => quote! { FireworkShape::Burst },
        other => panic!("Unknown firework explosion shape: {other}"),
    }
}

/// Generate the LootType enum variant at build time.
pub(super) fn generate_loot_type(loot_type: &str) -> TokenStream {
    let loot_type = if loot_type.contains(':') {
        loot_type.to_string()
    } else {
        format!("minecraft:{loot_type}")
    };

    match loot_type.as_str() {
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

fn generate_tool_predicate_from_predicates(predicates: &ToolPredicatesJson) -> Option<TokenStream> {
    if let Some(enchants) = &predicates.enchantments
        && let Some(first) = enchants.first()
        && let Some(enchant_name) = &first.enchantments
    {
        let enchant_name = enchant_name.strip_prefix("#minecraft:").unwrap_or(
            enchant_name
                .strip_prefix("minecraft:")
                .unwrap_or(enchant_name),
        );
        let min_level = first.levels.as_ref().and_then(|l| l.min).unwrap_or(1);
        return Some(quote! {
            ToolPredicate::HasEnchantment {
                enchantment: Identifier::vanilla_static(#enchant_name),
                min_level: #min_level,
            }
        });
    }

    if let Some(custom_data) = &predicates.custom_data {
        let tag = parse_lenient_compound(custom_data, "custom_data predicate");
        let tag = generate_nbt_compound(&tag);
        return Some(quote! {
            ToolPredicate::CustomData {
                tag: || {
                    let Some(data) = crate::data_components::CustomData::try_from_compound(#tag) else {
                        panic!("generated custom_data predicate is invalid");
                    };
                    data
                },
            }
        });
    }

    None
}

pub(super) fn generate_tool_predicate_from_item_predicate(pred: &ToolPredicateJson) -> TokenStream {
    if let Some(item_str) = &pred.items {
        if item_str.starts_with('#') {
            let tag = item_str
                .strip_prefix("#minecraft:")
                .unwrap_or(item_str.strip_prefix('#').unwrap_or(item_str));
            return quote! { ToolPredicate::Tag(Identifier::vanilla_static(#tag)) };
        }
        let item = generate_static_identifier_from_str(item_str, "loot");
        return quote! { ToolPredicate::Item(#item) };
    }

    if let Some(predicates) = &pred.predicates
        && let Some(generated) = generate_tool_predicate_from_predicates(predicates)
    {
        return generated;
    }

    quote! { ToolPredicate::Any }
}

pub(super) fn generate_tool_predicate(predicate: &Option<PredicateJson>) -> TokenStream {
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

    generate_tool_predicate_from_item_predicate(pred)
}

pub(super) fn generate_enchantment_options(
    options: &Option<EnchantmentOptionsJson>,
) -> TokenStream {
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

pub(super) fn generate_instrument_ref(value: &str) -> TokenStream {
    let id = Identifier::from_str(value)
        .unwrap_or_else(|error| panic!("invalid instrument id {value:?}: {error}"));
    assert_eq!(
        id.namespace.as_ref(),
        "minecraft",
        "vanilla loot table references a non-vanilla instrument: {id}"
    );
    let ident = Ident::new(&id.path.to_shouty_snake_case(), Span::call_site());
    quote! { &crate::vanilla_instruments::#ident }
}

pub(super) fn generate_instrument_options(options: &Option<EnchantmentOptionsJson>) -> TokenStream {
    match options {
        Some(EnchantmentOptionsJson::Tag(value)) if value.starts_with('#') => {
            let tag = value.trim_start_matches('#');
            let id = Identifier::from_str(tag)
                .unwrap_or_else(|error| panic!("invalid instrument tag {value:?}: {error}"));
            let namespace = id.namespace.as_ref();
            let path = id.path.as_ref();
            quote! {
                InstrumentOptions::Tag(Identifier::new_static(#namespace, #path))
            }
        }
        Some(EnchantmentOptionsJson::Tag(value)) => {
            let instrument = generate_instrument_ref(value);
            quote! { InstrumentOptions::Direct(&[#instrument]) }
        }
        Some(EnchantmentOptionsJson::List(values)) => {
            let instruments = values.iter().map(|value| generate_instrument_ref(value));
            quote! { InstrumentOptions::Direct(&[#(#instruments),*]) }
        }
        None => panic!("set_instrument function is missing its options holder set"),
    }
}

pub(super) fn generate_entity_flags(flags: &Option<EntityFlagsJson>) -> TokenStream {
    if let Some(f) = flags {
        let is_on_fire = if let Some(v) = f.is_on_fire {
            quote! { Some(#v) }
        } else {
            quote! { None }
        };
        let is_sneaking = if let Some(v) = f.is_sneaking {
            quote! { Some(#v) }
        } else {
            quote! { None }
        };
        let is_sprinting = if let Some(v) = f.is_sprinting {
            quote! { Some(#v) }
        } else {
            quote! { None }
        };
        let is_swimming = if let Some(v) = f.is_swimming {
            quote! { Some(#v) }
        } else {
            quote! { None }
        };
        let is_baby = if let Some(v) = f.is_baby {
            quote! { Some(#v) }
        } else {
            quote! { None }
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
    } else {
        quote! { None }
    }
}

pub(super) fn generate_equipment_slot_predicate(slot: &Option<EquipmentSlotJson>) -> TokenStream {
    match slot {
        Some(s) => {
            if let Some(items) = &s.items {
                if items.starts_with('#') {
                    let tag = items
                        .strip_prefix("#minecraft:")
                        .unwrap_or(items.strip_prefix('#').unwrap_or(items));
                    return quote! { Some(ToolPredicate::Tag(Identifier::vanilla_static(#tag))) };
                }
                let item = generate_static_identifier_from_str(items, "loot");
                return quote! { Some(ToolPredicate::Item(#item)) };
            }

            if let Some(predicates) = &s.predicates
                && let Some(generated) = generate_tool_predicate_from_predicates(predicates)
            {
                return quote! { Some(#generated) };
            }

            quote! { Some(ToolPredicate::Any) }
        }
        None => quote! { None },
    }
}

pub(super) fn generate_entity_equipment(equipment: &Option<EntityEquipmentJson>) -> TokenStream {
    if let Some(e) = equipment {
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
    } else {
        quote! { None }
    }
}

pub(super) fn generate_entity_predicate(predicate: &EntityPredicateJson) -> TokenStream {
    let entity_type = if let Some(t) = &predicate.entity_type {
        let t = t.strip_prefix("minecraft:").unwrap_or(t);
        quote! { Some(Identifier::vanilla_static(#t)) }
    } else {
        quote! { None }
    };

    let flags = generate_entity_flags(&predicate.flags);
    let equipment = generate_entity_equipment(&predicate.equipment);

    let sheep_color = predicate
        .components
        .as_ref()
        .and_then(|components| components.sheep_color.as_deref())
        .map_or_else(
            || quote! { None },
            |color| {
                let color = generate_dye_color(color);
                quote! { Some(#color) }
            },
        );
    let chicken_variant = predicate
        .components
        .as_ref()
        .and_then(|components| components.chicken_variant.as_deref())
        .map_or_else(
            || quote! { None },
            |variant| {
                let variant = variant.strip_prefix("minecraft:").unwrap_or(variant);
                quote! { Some(Identifier::vanilla_static(#variant)) }
            },
        );
    let sheep_sheared = predicate
        .sheep_type_specific
        .as_ref()
        .and_then(|sheep| sheep.sheared)
        .map_or_else(|| quote! { None }, |sheared| quote! { Some(#sheared) });

    quote! {
        EntityPredicate {
            entity_type: #entity_type,
            flags: #flags,
            equipment: #equipment,
            sheep_color: #sheep_color,
            sheep_sheared: #sheep_sheared,
            chicken_variant: #chicken_variant,
        }
    }
}

pub(super) fn generate_damage_source_predicate(
    predicate: &DamageSourcePredicateJson,
) -> TokenStream {
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

    let source_entity = if let Some(e) = &predicate.source_entity {
        let pred = generate_entity_predicate(e);
        quote! { Some(#pred) }
    } else {
        quote! { None }
    };

    let direct_entity = if let Some(e) = &predicate.direct_entity {
        let pred = generate_entity_predicate(e);
        quote! { Some(#pred) }
    } else {
        quote! { None }
    };

    let is_direct = if let Some(v) = predicate.is_direct {
        quote! { Some(#v) }
    } else {
        quote! { None }
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

pub(super) fn generate_block_predicate(predicate: &BlockPredicateJson) -> TokenStream {
    let blocks = if let Some(b) = &predicate.blocks {
        let b = b.strip_prefix("minecraft:").unwrap_or(b);
        quote! { Some(Identifier::vanilla_static(#b)) }
    } else {
        quote! { None }
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

pub(super) fn generate_location_predicate(predicate: &LocationPredicateJson) -> TokenStream {
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
