use crate::generator_functions::{read_json_asset, sort_contiguous_registry_entries};
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize)]
struct MobEffectEntry {
    id: u16,
    name: String,
    category: MobEffectCategoryEntry,
    color: i32,
    particle: MobEffectParticleEntry,
    #[serde(default)]
    attribute_modifiers: Vec<MobEffectAttributeModifierEntry>,
}

#[derive(Deserialize)]
struct MobEffectParticleEntry {
    #[serde(rename = "type")]
    particle_type: Identifier,
    options_type: MobEffectParticleOptionsType,
    regular_alpha: Option<u8>,
    ambient_alpha: Option<u8>,
    color: Option<i32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum MobEffectParticleOptionsType {
    MobEffectColor,
    Simple,
    FixedColor,
}

#[derive(Deserialize)]
struct MobEffectAttributeModifierEntry {
    attribute: String,
    id: String,
    amount: f64,
    operation: AttributeModifierOperationEntry,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum MobEffectCategoryEntry {
    Beneficial,
    Harmful,
    Neutral,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[expect(
    clippy::enum_variant_names,
    reason = "build input mirrors vanilla AttributeModifier.Operation names"
)]
enum AttributeModifierOperationEntry {
    AddValue,
    AddMultipliedBase,
    AddMultipliedTotal,
}

impl AttributeModifierOperationEntry {
    fn token(&self) -> TokenStream {
        match self {
            Self::AddValue => quote! { AttributeModifierOperation::AddValue },
            Self::AddMultipliedBase => quote! { AttributeModifierOperation::AddMultipliedBase },
            Self::AddMultipliedTotal => quote! { AttributeModifierOperation::AddMultipliedTotal },
        }
    }
}

impl MobEffectCategoryEntry {
    fn token(&self) -> TokenStream {
        match self {
            Self::Beneficial => quote! { MobEffectCategory::Beneficial },
            Self::Harmful => quote! { MobEffectCategory::Harmful },
            Self::Neutral => quote! { MobEffectCategory::Neutral },
        }
    }
}

pub(crate) fn build() -> TokenStream {
    const ASSET: &str = "build_assets/mob_effects.json";

    let mut effects: Vec<MobEffectEntry> = read_json_asset(ASSET);
    sort_contiguous_registry_entries(&mut effects, ASSET, |effect| usize::from(effect.id));

    let mut constants = TokenStream::new();
    let mut registrations = TokenStream::new();
    let mut modifier_constants = TokenStream::new();

    for effect in &effects {
        let ident = Ident::new(&effect.name.to_shouty_snake_case(), Span::call_site());
        let modifiers_ident = Ident::new(
            &format!("{}_ATTRIBUTE_MODIFIERS", effect.name.to_shouty_snake_case()),
            Span::call_site(),
        );
        let key = Literal::string(&effect.name);
        let category = effect.category.token();
        let color = effect.color;
        let particle_ident = Ident::new(
            &effect.particle.particle_type.path.to_shouty_snake_case(),
            Span::call_site(),
        );
        let particle = match effect.particle.options_type {
            MobEffectParticleOptionsType::MobEffectColor => {
                let regular_alpha = effect.particle.regular_alpha.unwrap_or_else(|| {
                    panic!(
                        "mob effect '{}' color particle is missing regular_alpha",
                        effect.name
                    )
                });
                let ambient_alpha = effect.particle.ambient_alpha.unwrap_or_else(|| {
                    panic!(
                        "mob effect '{}' color particle is missing ambient_alpha",
                        effect.name
                    )
                });
                quote! {
                    MobEffectParticle::EffectColor {
                        particle_type: &vanilla_particle_types::#particle_ident,
                        regular_alpha: #regular_alpha,
                        ambient_alpha: #ambient_alpha,
                    }
                }
            }
            MobEffectParticleOptionsType::Simple => {
                quote! { MobEffectParticle::Simple(&vanilla_particle_types::#particle_ident) }
            }
            MobEffectParticleOptionsType::FixedColor => {
                let particle_color = effect.particle.color.unwrap_or_else(|| {
                    panic!(
                        "mob effect '{}' fixed color particle is missing color",
                        effect.name
                    )
                });
                quote! {
                    MobEffectParticle::FixedColor {
                        particle_type: &vanilla_particle_types::#particle_ident,
                        color: ArgbColor::new(#particle_color),
                    }
                }
            }
        };
        let mut modifier_entries = TokenStream::new();

        for modifier in &effect.attribute_modifiers {
            let attribute_ident = Ident::new(
                &modifier.attribute.to_shouty_snake_case(),
                Span::call_site(),
            );
            let modifier_id = Literal::string(&modifier.id);
            let amount = modifier.amount;
            let operation = modifier.operation.token();
            modifier_entries.extend(quote! {
                MobEffectAttributeModifier {
                    attribute: vanilla_attributes::#attribute_ident,
                    id: Identifier::vanilla_static(#modifier_id),
                    amount: #amount,
                    operation: #operation,
                },
            });
        }

        modifier_constants.extend(quote! {
            static #modifiers_ident: &[MobEffectAttributeModifier] = &[
                #modifier_entries
            ];
        });

        constants.extend(quote! {
            pub static #ident: &MobEffect = &MobEffect {
                key: Identifier::vanilla_static(#key),
                category: #category,
                color: RgbColor::new(#color),
                particle: #particle,
                attribute_modifiers: #modifiers_ident,
            };
        });

        registrations.extend(quote! {
            registry.register(#ident);
        });
    }

    quote! {
        use crate::attribute::AttributeModifierOperation;
        use crate::mob_effect::{
            MobEffect, MobEffectAttributeModifier, MobEffectCategory, MobEffectParticle,
            MobEffectRegistry,
        };
        use crate::{vanilla_attributes, vanilla_particle_types};
        use steel_utils::{ArgbColor, Identifier, RgbColor};

        #modifier_constants
        #constants

        pub fn register_mob_effects(registry: &mut MobEffectRegistry) {
            #registrations
        }
    }
}
