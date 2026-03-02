use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct DamageTypeJson {
    message_id: String,
    scaling: DamageScalingJson,
    exhaustion: f32,
    #[serde(default)]
    effects: DamageEffectsJson,
    #[serde(default)]
    death_message_type: DeathMessageTypeJson,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DamageScalingJson {
    Always,
    WhenCausedByLivingNonPlayer,
    Never,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DamageEffectsJson {
    #[default]
    Hurt,
    Thorns,
    Drowning,
    Burning,
    Poking,
    Freezing,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeathMessageTypeJson {
    #[default]
    Default,
    FallVariants,
    IntentionalGameDesign,
}

fn generate_damage_scaling(scaling: DamageScalingJson) -> TokenStream {
    match scaling {
        DamageScalingJson::Always => quote! { DamageScaling::Always },
        DamageScalingJson::WhenCausedByLivingNonPlayer => {
            quote! { DamageScaling::WhenCausedByLivingNonPlayer }
        }
        DamageScalingJson::Never => quote! { DamageScaling::Never },
    }
}

fn generate_damage_effects(effects: DamageEffectsJson) -> TokenStream {
    match effects {
        DamageEffectsJson::Hurt => quote! { DamageEffects::Hurt },
        DamageEffectsJson::Thorns => quote! { DamageEffects::Thorns },
        DamageEffectsJson::Drowning => quote! { DamageEffects::Drowning },
        DamageEffectsJson::Burning => quote! { DamageEffects::Burning },
        DamageEffectsJson::Poking => quote! { DamageEffects::Poking },
        DamageEffectsJson::Freezing => quote! { DamageEffects::Freezing },
    }
}

fn generate_death_message_type(death_message_type: DeathMessageTypeJson) -> TokenStream {
    match death_message_type {
        DeathMessageTypeJson::Default => quote! { DeathMessageType::Default },
        DeathMessageTypeJson::FallVariants => quote! { DeathMessageType::FallVariants },
        DeathMessageTypeJson::IntentionalGameDesign => {
            quote! { DeathMessageType::IntentionalGameDesign }
        }
    }
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/damage_type/"
    );

    let damage_type_dir = "build_assets/builtin_datapacks/minecraft/data/minecraft/damage_type";
    let mut damage_types = Vec::new();

    // Read all damage type JSON files
    for entry in fs::read_dir(damage_type_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let damage_type_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let damage_type: DamageTypeJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", damage_type_name, e));

            damage_types.push((damage_type_name, damage_type));
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::damage_type::{
            DamageType, DamageTypeRegistry, DamageScaling, DamageEffects, DeathMessageType,
        };
        use steel_utils::Identifier;
    });

    // Generate static damage type definitions
    let mut register_stream = TokenStream::new();
    for (damage_type_name, damage_type) in &damage_types {
        let damage_type_ident =
            Ident::new(&damage_type_name.to_shouty_snake_case(), Span::call_site());
        let damage_type_name_str = damage_type_name.clone();

        let key = quote! { Identifier::vanilla_static(#damage_type_name_str) };
        let message_id = damage_type.message_id.as_str();
        let scaling = generate_damage_scaling(damage_type.scaling);
        let exhaustion = damage_type.exhaustion;
        let effects = generate_damage_effects(damage_type.effects);
        let death_message_type = generate_death_message_type(damage_type.death_message_type);

        stream.extend(quote! {
            pub static #damage_type_ident: &DamageType = &DamageType {
                key: #key,
                message_id: #message_id,
                scaling: #scaling,
                exhaustion: #exhaustion,
                effects: #effects,
                death_message_type: #death_message_type,
            };
        });
        register_stream.extend(quote! {
            registry.register(#damage_type_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_damage_types(registry: &mut DamageTypeRegistry) {
            #register_stream
        }
    });

    stream
}
