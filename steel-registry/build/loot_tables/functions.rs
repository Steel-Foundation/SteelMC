use super::{
    LimitJson, LootFunctionJson, TokenStream, generate_condition, generate_enchantment_options,
    generate_instrument_options, generate_number_provider, quote,
};

pub(super) fn generate_function(function: &LootFunctionJson) -> TokenStream {
    let func_body = match function.function.as_str() {
        "minecraft:set_count" => {
            let count = function.count.as_ref().map_or_else(
                || quote! { NumberProvider::Constant(1.0) },
                generate_number_provider,
            );
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

            let count = function.count.as_ref().map_or_else(
                || quote! { NumberProvider::Uniform { min: 0.0, max: 1.0 } },
                generate_number_provider,
            );

            let limit = match &function.limit {
                Some(LimitJson::Integer(v)) => *v,
                Some(LimitJson::Object { max, .. }) => max.map_or(0, |v| v as i32),
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

            let min_tokens = if let Some(v) = min {
                quote! { Some(#v) }
            } else {
                quote! { None }
            };
            let max_tokens = if let Some(v) = max {
                quote! { Some(#v) }
            } else {
                quote! { None }
            };

            quote! { LootFunction::LimitCount { min: #min_tokens, max: #max_tokens } }
        }
        "minecraft:set_damage" => {
            let damage = function.damage.as_ref().map_or_else(
                || quote! { NumberProvider::Constant(1.0) },
                generate_number_provider,
            );
            let add = function.add;
            quote! { LootFunction::SetDamage { damage: #damage, add: #add } }
        }
        "minecraft:enchant_randomly" => {
            let options = generate_enchantment_options(&function.options);
            quote! { LootFunction::EnchantRandomly { options: #options } }
        }
        "minecraft:enchant_with_levels" => {
            let levels = function.levels.as_ref().map_or_else(
                || quote! { NumberProvider::Constant(30.0) },
                generate_number_provider,
            );
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
                .map_or_else(|| "{}".to_string(), std::string::ToString::to_string);
            quote! { LootFunction::SetComponents { components: #components_str } }
        }
        "minecraft:furnace_smelt" => {
            let use_input_count = function.use_input_count.unwrap_or(true);
            quote! { LootFunction::FurnaceSmelt { use_input_count: #use_input_count } }
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
                .map_or_else(|| "\"\"".to_string(), std::string::ToString::to_string);

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
            let amplifier = function.amplifier.as_ref().map_or_else(
                || quote! { NumberProvider::Constant(0.0) },
                generate_number_provider,
            );
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
            let options = generate_instrument_options(&function.options);
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
            panic!("Unknown loot function type: {other}");
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
