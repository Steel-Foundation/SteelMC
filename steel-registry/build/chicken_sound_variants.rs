use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
pub struct Age {
    ambient_sound: Identifier,
    death_sound: Identifier,
    hurt_sound: Identifier,
    step_sound: Identifier,
}
#[derive(Deserialize, Debug)]
pub struct ChickenSoundVariantJson {
    adult_sounds: Age,
    baby_sounds: Age,
}

fn generate_identifier(resource: &Identifier) -> TokenStream {
    let path = resource.path.as_ref();
    quote! { Identifier::vanilla_static(#path) }
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/chicken_sound_variant/"
    );

    let chicken_sound_variant_dir =
        "build_assets/builtin_datapacks/minecraft/data/minecraft/chicken_sound_variant";
    let mut chicken_sound_variants = Vec::new();

    // Read all chicken sound variant JSON files
    for entry in fs::read_dir(chicken_sound_variant_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let chicken_sound_variant_name =
                path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let chicken_sound_variant: ChickenSoundVariantJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| {
                    panic!("Failed to parse {}: {}", chicken_sound_variant_name, e)
                });

            chicken_sound_variants.push((chicken_sound_variant_name, chicken_sound_variant));
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::chicken_sound_variant::{
            ChickenSoundVariant, ChickenSoundVariantRegistry, ChickenAge,
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;
    });

    // Generate static chicken sound variant definitions
    let mut register_stream = TokenStream::new();
    for (chicken_sound_variant_name, chicken_sound_variant) in &chicken_sound_variants {
        let chicken_sound_variant_ident = Ident::new(
            &chicken_sound_variant_name.to_shouty_snake_case(),
            Span::call_site(),
        );
        let chicken_sound_variant_name_str = chicken_sound_variant_name.clone();

        let key = quote! { Identifier::vanilla_static(#chicken_sound_variant_name_str) };
        let adult_ambient_sound =
            generate_identifier(&chicken_sound_variant.adult_sounds.ambient_sound);
        let adult_death_sound =
            generate_identifier(&chicken_sound_variant.adult_sounds.death_sound);
        let adult_hurt_sound = generate_identifier(&chicken_sound_variant.adult_sounds.hurt_sound);
        let adult_step_sound = generate_identifier(&chicken_sound_variant.adult_sounds.step_sound);
        let baby_ambient_sound =
            generate_identifier(&chicken_sound_variant.baby_sounds.ambient_sound);
        let baby_death_sound = generate_identifier(&chicken_sound_variant.baby_sounds.death_sound);
        let baby_hurt_sound = generate_identifier(&chicken_sound_variant.baby_sounds.hurt_sound);
        let baby_step_sound = generate_identifier(&chicken_sound_variant.baby_sounds.step_sound);

        stream.extend(quote! {
            pub static #chicken_sound_variant_ident: &ChickenSoundVariant = &ChickenSoundVariant {
                key: #key,
                adult_sounds: ChickenAge {
                    ambient_sound: #adult_ambient_sound,
                    death_sound: #adult_death_sound,
                    hurt_sound: #adult_hurt_sound,
                    step_sound: #adult_step_sound,
                },
                baby_sounds: ChickenAge {
                    ambient_sound: #baby_ambient_sound,
                    death_sound: #baby_death_sound,
                    hurt_sound: #baby_hurt_sound,
                    step_sound: #baby_step_sound,
                }
            };
        });

        register_stream.extend(quote! {
            registry.register(#chicken_sound_variant_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_chicken_sound_variants(registry: &mut ChickenSoundVariantRegistry) {
            #register_stream
        }
    });

    stream
}
