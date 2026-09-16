use super::{
    EnchantedChanceJson, LootConditionJson, PredicateJson, PropertyValueJson, TokenStream,
    generate_damage_source_predicate, generate_entity_predicate, generate_location_predicate,
    generate_loot_context_entity, generate_tool_predicate, quote,
};

pub(super) fn generate_condition(condition: &LootConditionJson) -> TokenStream {
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
                if let PredicateJson::Entity(e) = pred {
                    generate_entity_predicate(e)
                } else {
                    quote! {
                        EntityPredicate {
                            entity_type: None,
                            flags: None,
                            equipment: None,
                            sheep_color: None,
                            sheep_sheared: None,
                            chicken_variant: None,
                        }
                    }
                }
            } else {
                quote! {
                    EntityPredicate {
                        entity_type: None,
                        flags: None,
                        equipment: None,
                        sheep_color: None,
                        sheep_sheared: None,
                        chicken_variant: None,
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
                if let PredicateJson::DamageSource(ds) = pred {
                    generate_damage_source_predicate(ds)
                } else {
                    quote! {
                        DamageSourcePredicate {
                            tags: &[],
                            source_entity: None,
                            direct_entity: None,
                            is_direct: None,
                        }
                    }
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
                if let PredicateJson::Location(l) = pred {
                    generate_location_predicate(l)
                } else {
                    quote! {
                        LocationPredicate {
                            block: None,
                        }
                    }
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
            panic!("Unknown loot condition type: {other}");
        }
    }
}
