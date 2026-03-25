use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
pub struct PigAge {
    ambient_sound: Identifier,
    death_sound: Identifier,
    eat_sound: Identifier,
    hurt_sound: Identifier,
    step_sound: Identifier,
}
#[derive(Deserialize, Debug)]
pub struct PigSoundVariantJson {
    adult_sounds: PigAge,
    baby_sounds: PigAge,
}

fn generate_identifier(resource: &Identifier) -> TokenStream {
    let path = resource.path.as_ref();
    quote! { Identifier::vanilla_static(#path) }
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/pig_sound_variant/"
    );

    let pig_sound_variant_dir =
        "build_assets/builtin_datapacks/minecraft/data/minecraft/pig_sound_variant";
    let mut pig_sound_variants = Vec::new();

    // Read all pig sound variant JSON files
    for entry in fs::read_dir(pig_sound_variant_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let pig_sound_variant_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let pig_sound_variant: PigSoundVariantJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", pig_sound_variant_name, e));

            pig_sound_variants.push((pig_sound_variant_name, pig_sound_variant));
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::pig_sound_variant::{
            PigSoundVariant, PigSoundVariantRegistry, PigAge
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;
    });

    // Generate static pig sound variant definitions
    let mut register_stream = TokenStream::new();
    for (pig_sound_variant_name, pig_sound_variant) in &pig_sound_variants {
        let pig_sound_variant_ident = Ident::new(
            &pig_sound_variant_name.to_shouty_snake_case(),
            Span::call_site(),
        );
        let pig_sound_variant_name_str = pig_sound_variant_name.clone();

        let key = quote! { Identifier::vanilla_static(#pig_sound_variant_name_str) };
        let adult_ambient_sound =
            generate_identifier(&pig_sound_variant.adult_sounds.ambient_sound);
        let adult_death_sound = generate_identifier(&pig_sound_variant.adult_sounds.death_sound);
        let adult_hurt_sound = generate_identifier(&pig_sound_variant.adult_sounds.hurt_sound);
        let adult_eat_sound = generate_identifier(&pig_sound_variant.adult_sounds.eat_sound);
        let adult_step_sound = generate_identifier(&pig_sound_variant.adult_sounds.step_sound);
        let baby_ambient_sound = generate_identifier(&pig_sound_variant.baby_sounds.ambient_sound);
        let baby_death_sound = generate_identifier(&pig_sound_variant.baby_sounds.death_sound);
        let baby_hurt_sound = generate_identifier(&pig_sound_variant.baby_sounds.hurt_sound);
        let baby_step_sound = generate_identifier(&pig_sound_variant.baby_sounds.step_sound);
        let baby_eat_sound = generate_identifier(&pig_sound_variant.baby_sounds.eat_sound);

        stream.extend(quote! {
            pub static #pig_sound_variant_ident: &PigSoundVariant = &PigSoundVariant {
                key: #key,
                adult_sounds: PigAge {
                    ambient_sound: #adult_ambient_sound,
                    death_sound: #adult_death_sound,
                    hurt_sound: #adult_hurt_sound,
                    step_sound: #adult_step_sound,
                    eat_sound: #adult_eat_sound
                },
                baby_sounds: PigAge {
                    ambient_sound: #baby_ambient_sound,
                    death_sound: #baby_death_sound,
                    hurt_sound: #baby_hurt_sound,
                    step_sound: #baby_step_sound,
                    eat_sound: #baby_eat_sound,
                }
            };
        });

        register_stream.extend(quote! {
            registry.register(#pig_sound_variant_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_pig_sound_variants(registry: &mut PigSoundVariantRegistry) {
            #register_stream
        }
    });

    stream
}
