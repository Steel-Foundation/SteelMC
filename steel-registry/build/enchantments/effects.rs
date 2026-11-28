use std::sync::OnceLock;

use crate::particle_types::{self, ParticleOptionsType};

use super::{
    AttributeEffectJson, ConditionalDamageImmunityEffectJson, ConditionalEntityEffectJson,
    ConditionalValueEffectJson, CrossbowChargingSoundsJson, DamageSourcePredicateJson,
    DoubleBoundsJson, EnchantmentTargetJson, EntityEffectJson, EntityFlagsPredicateJson,
    EntityPredicateJson, EntityTargetJson, EntityTypePredicateJson,
    EntityTypeSpecificPredicateJson, EntityVehiclePredicateJson, ExplodeEffectJson,
    ExplosionInteractionJson, GameType, Ident, Identifier, ItemHolderSetJson, LevelBasedValueJson,
    LevelBasedValueTypedJson, MobEffectSelectionJson, MovementPredicateJson, RequirementsJson,
    Span, TargetedConditionalEntityEffectJson, TargetedConditionalValueEffectJson,
    ToShoutySnakeCase, TokenStream, ValueEffectJson, damage_type_ref_token,
    generate_sound_event_ref, identifier_token, quote,
};

pub(super) fn attribute_ref_token(attribute: &Identifier) -> TokenStream {
    assert_eq!(
        attribute.namespace.as_ref(),
        "minecraft",
        "vanilla enchantment attribute references must use the minecraft namespace: {attribute}"
    );
    let ident = Ident::new(&attribute.path.to_shouty_snake_case(), Span::call_site());
    quote! { vanilla_attributes::#ident }
}

pub(super) fn mob_effect_ref_token(effect: &Identifier) -> TokenStream {
    assert_eq!(
        effect.namespace.as_ref(),
        "minecraft",
        "vanilla enchantment mob effect references must use the minecraft namespace: {effect}"
    );
    let ident = Ident::new(&effect.path.to_shouty_snake_case(), Span::call_site());
    quote! { vanilla_mob_effects::#ident }
}

pub(super) fn attribute_modifier_operation_token(operation: &str) -> TokenStream {
    match operation {
        "add_value" => quote! { AttributeModifierOperation::AddValue },
        "add_multiplied_base" => quote! { AttributeModifierOperation::AddMultipliedBase },
        "add_multiplied_total" => quote! { AttributeModifierOperation::AddMultipliedTotal },
        other => panic!("Unknown enchantment attribute modifier operation: {other}"),
    }
}

pub(super) fn option_sound_event_ref_token(sound: Option<&Identifier>) -> TokenStream {
    if let Some(sound) = sound {
        let sound = generate_sound_event_ref(sound);
        quote! { Some(#sound) }
    } else {
        quote! { None }
    }
}

pub(super) fn generate_level_based_value_ref(
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

pub(super) fn generate_level_based_value(
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

pub(super) fn generate_value_effect(
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

pub(super) fn entity_target_token(entity: &EntityTargetJson) -> TokenStream {
    match entity {
        EntityTargetJson::This => quote! { EnchantmentEntityTarget::This },
        EntityTargetJson::Attacker => quote! { EnchantmentEntityTarget::Attacker },
        EntityTargetJson::DirectAttacker => quote! { EnchantmentEntityTarget::DirectAttacker },
    }
}

pub(super) fn entity_type_predicate_token(predicate: &EntityTypePredicateJson) -> TokenStream {
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

pub(super) fn game_type_token(game_type: GameType) -> TokenStream {
    match game_type {
        GameType::Survival => quote! { GameType::Survival },
        GameType::Creative => quote! { GameType::Creative },
        GameType::Adventure => quote! { GameType::Adventure },
        GameType::Spectator => quote! { GameType::Spectator },
    }
}

pub(super) fn entity_vehicle_predicate_token(
    predicate: &EntityVehiclePredicateJson,
) -> TokenStream {
    match predicate {
        EntityVehiclePredicateJson::Any => quote! { EntityVehiclePredicate::Any },
        EntityVehiclePredicateJson::Present => quote! { EntityVehiclePredicate::Present },
        EntityVehiclePredicateJson::Unsupported => quote! { EntityVehiclePredicate::Unsupported },
    }
}

fn option_bool_token(value: Option<bool>) -> TokenStream {
    if let Some(value) = value {
        quote! { Some(#value) }
    } else {
        quote! { None }
    }
}

pub(super) fn entity_flags_predicate_token(predicate: &EntityFlagsPredicateJson) -> TokenStream {
    let is_flying = option_bool_token(predicate.is_flying);
    let is_fall_flying = option_bool_token(predicate.is_fall_flying);
    let is_in_water = option_bool_token(predicate.is_in_water);
    let unsupported = predicate.unsupported;

    quote! {
        EntityFlagsPredicate {
            is_flying: #is_flying,
            is_fall_flying: #is_fall_flying,
            is_in_water: #is_in_water,
            unsupported: #unsupported,
        }
    }
}

fn double_bounds_token(bounds: DoubleBoundsJson) -> TokenStream {
    match (bounds.min, bounds.max) {
        (None, None) => quote! { DoubleBounds::ANY },
        (Some(min), None) => quote! { DoubleBounds::at_least(#min) },
        (None, Some(max)) => quote! { DoubleBounds::at_most(#max) },
        (Some(min), Some(max)) => quote! { DoubleBounds::between(#min, #max) },
    }
}

fn movement_predicate_ref_token(
    prefix: &str,
    movement: Option<&MovementPredicateJson>,
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    let Some(movement) = movement else {
        return quote! { None };
    };
    let ident = Ident::new(
        &format!("{prefix}_MOVEMENT_PREDICATE_{}", *counter),
        Span::call_site(),
    );
    *counter += 1;
    let x = double_bounds_token(movement.x);
    let y = double_bounds_token(movement.y);
    let z = double_bounds_token(movement.z);
    let speed = double_bounds_token(movement.speed);
    let horizontal_speed = double_bounds_token(movement.horizontal_speed);
    let vertical_speed = double_bounds_token(movement.vertical_speed);
    let fall_distance = double_bounds_token(movement.fall_distance);
    statics.extend(quote! {
        static #ident: MovementPredicate = MovementPredicate {
            x: #x,
            y: #y,
            z: #z,
            speed: #speed,
            horizontal_speed: #horizontal_speed,
            vertical_speed: #vertical_speed,
            fall_distance: #fall_distance,
        };
    });
    quote! { Some(&#ident) }
}

pub(super) fn entity_type_specific_predicate_token(
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

pub(super) fn entity_predicate_token(
    prefix: &str,
    predicate: &EntityPredicateJson,
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    let entity_type = entity_type_predicate_token(&predicate.entity_type);
    let vehicle = entity_vehicle_predicate_token(&predicate.vehicle);
    let flags = entity_flags_predicate_token(&predicate.flags);
    let movement =
        movement_predicate_ref_token(prefix, predicate.movement.as_deref(), statics, counter);
    let type_specific = entity_type_specific_predicate_token(&predicate.type_specific);
    let unsupported = predicate.unsupported;
    quote! {
        EntityPredicate {
            entity_type: #entity_type,
            vehicle: #vehicle,
            flags: #flags,
            movement: #movement,
            type_specific: #type_specific,
            unsupported: #unsupported,
        }
    }
}

pub(super) fn damage_source_predicate_token(predicate: &DamageSourcePredicateJson) -> TokenStream {
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

pub(super) fn enchantment_target_token(target: &EnchantmentTargetJson) -> TokenStream {
    match target {
        EnchantmentTargetJson::Attacker => quote! { EnchantmentTarget::Attacker },
        EnchantmentTargetJson::DamagingEntity => quote! { EnchantmentTarget::DamagingEntity },
        EnchantmentTargetJson::Victim => quote! { EnchantmentTarget::Victim },
    }
}

pub(super) fn mob_effect_selection_token(selection: &MobEffectSelectionJson) -> TokenStream {
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

pub(super) fn generate_entity_effect_ref(
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

pub(super) fn generate_entity_effect(
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
        EntityEffectJson::Explode(explode) => {
            let explode = generate_explode_effect(prefix, explode, statics, counter);
            quote! { EnchantmentEntityEffect::Explode(#explode) }
        }
        EntityEffectJson::Unsupported { effect_type } => {
            let effect_type = identifier_token(effect_type);
            quote! { EnchantmentEntityEffect::Unsupported { effect_type: #effect_type } }
        }
    }
}

fn generate_explode_effect(
    prefix: &str,
    explode: &ExplodeEffectJson,
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    let attribute_to_user = explode.attribute_to_user;
    let damage_type = if let Some(damage_type) = &explode.damage_type {
        let damage_type = damage_type_ref_token(damage_type);
        quote! { Some(#damage_type) }
    } else {
        quote! { None }
    };
    let knockback_multiplier = if let Some(multiplier) = &explode.knockback_multiplier {
        let multiplier = generate_level_based_value_ref(prefix, multiplier, statics, counter);
        quote! { Some(#multiplier) }
    } else {
        quote! { None }
    };
    let immune_blocks = if let Some(tag) = &explode.immune_blocks_tag {
        let tag = identifier_token(tag);
        quote! { Some(#tag) }
    } else {
        quote! { None }
    };
    let [offset_x, offset_y, offset_z] = explode.offset;
    let radius = generate_level_based_value_ref(prefix, &explode.radius, statics, counter);
    let create_fire = explode.create_fire;
    let block_interaction = match explode.block_interaction {
        ExplosionInteractionJson::None => quote! { ExplosionInteraction::None },
        ExplosionInteractionJson::Block => quote! { ExplosionInteraction::Block },
        ExplosionInteractionJson::Mob => quote! { ExplosionInteraction::Mob },
        ExplosionInteractionJson::Tnt => quote! { ExplosionInteraction::Tnt },
        ExplosionInteractionJson::Trigger => quote! { ExplosionInteraction::Trigger },
    };
    let small_particle = simple_particle_type_ref_token(&explode.small_particle);
    let large_particle = simple_particle_type_ref_token(&explode.large_particle);
    let sound = generate_sound_event_ref(&explode.sound);

    quote! {
        EnchantmentExplodeEffect {
            attribute_to_user: #attribute_to_user,
            damage_type: #damage_type,
            knockback_multiplier: #knockback_multiplier,
            immune_blocks: #immune_blocks,
            offset: DVec3::new(#offset_x, #offset_y, #offset_z),
            radius: #radius,
            create_fire: #create_fire,
            block_interaction: #block_interaction,
            small_particle: #small_particle,
            large_particle: #large_particle,
            sound: #sound,
        }
    }
}

/// Resolves a particle type that must not carry options, checked against the
/// extracted particle registry so runtime `ParticleData::simple` cannot panic.
fn simple_particle_type_ref_token(particle: &Identifier) -> TokenStream {
    static SIMPLE_PARTICLES: OnceLock<Vec<Identifier>> = OnceLock::new();
    let simple_particles = SIMPLE_PARTICLES.get_or_init(|| {
        particle_types::read_particle_types()
            .into_iter()
            .filter(|entry| matches!(entry.options_type, ParticleOptionsType::Simple))
            .map(|entry| entry.key)
            .collect()
    });
    assert!(
        simple_particles.contains(particle),
        "enchantment effect particle {particle} must be an option-less (simple) particle type"
    );
    assert_eq!(
        particle.namespace.as_ref(),
        "minecraft",
        "vanilla enchantment particle references must use the minecraft namespace: {particle}"
    );
    let ident = Ident::new(&particle.path.to_shouty_snake_case(), Span::call_site());
    quote! { &crate::vanilla_particle_types::#ident }
}

pub(super) fn generate_requirements_ref(
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

pub(super) fn generate_requirements_value(
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
            let predicate = entity_predicate_token(prefix, predicate, statics, counter);
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

pub(super) fn generate_optional_requirements(
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

pub(super) fn generate_conditional_value_effects(
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

pub(super) fn generate_conditional_entity_effects(
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

pub(super) fn generate_damage_immunity_effects(
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

pub(super) fn generate_targeted_entity_effects(
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

pub(super) fn generate_targeted_value_effects(
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

pub(super) fn generate_attribute_effects(
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

pub(super) fn generate_optional_value_effect(
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

pub(super) fn generate_crossbow_charging_sounds(
    sounds: &[CrossbowChargingSoundsJson],
) -> TokenStream {
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

pub(super) fn generate_sound_event_refs(sounds: &[Identifier]) -> TokenStream {
    let sounds = sounds.iter().map(generate_sound_event_ref);
    quote! { &[#(#sounds),*] }
}
