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
    growl_sound: Identifier,
    hurt_sound: Identifier,
    pant_sound: Identifier,
    step_sound: Identifier,
    whine_sound: Identifier,
}
#[derive(Deserialize, Debug)]
pub struct WolfSoundVariantJson {
    adult_sounds: Age,
    baby_sounds: Age,
}

fn generate_identifier(resource: &Identifier) -> TokenStream {
    let path = resource.path.as_ref();
    quote! { Identifier::vanilla_static(#path) }
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/wolf_sound_variant/");

    let wolf_sound_variant_dir = "build_assets/builtin_datapacks/minecraft/wolf_sound_variant";
    let mut wolf_sound_variants = Vec::new();

    // Read all wolf sound variant JSON files
    for entry in fs::read_dir(wolf_sound_variant_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let wolf_sound_variant_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let wolf_sound_variant: WolfSoundVariantJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", wolf_sound_variant_name, e));

            wolf_sound_variants.push((wolf_sound_variant_name, wolf_sound_variant));
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::wolf_sound_variant::{
            WolfSoundVariant, WolfSoundVariantRegistry, WolfAge
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;
    });

    // Generate static wolf sound variant definitions
    let mut register_stream = TokenStream::new();
    for (wolf_sound_variant_name, wolf_sound_variant) in &wolf_sound_variants {
        let wolf_sound_variant_ident = Ident::new(
            &wolf_sound_variant_name.to_shouty_snake_case(),
            Span::call_site(),
        );
        let wolf_sound_variant_name_str = wolf_sound_variant_name.clone();

        let key = quote! { Identifier::vanilla_static(#wolf_sound_variant_name_str) };
        let adult_ambient_sound =
            generate_identifier(&wolf_sound_variant.adult_sounds.ambient_sound);
        let adult_death_sound = generate_identifier(&wolf_sound_variant.adult_sounds.death_sound);
        let adult_growl_sound = generate_identifier(&wolf_sound_variant.adult_sounds.growl_sound);
        let adult_hurt_sound = generate_identifier(&wolf_sound_variant.adult_sounds.hurt_sound);
        let adult_pant_sound = generate_identifier(&wolf_sound_variant.adult_sounds.pant_sound);
        let adult_whine_sound = generate_identifier(&wolf_sound_variant.adult_sounds.whine_sound);
        let adult_step_sound = generate_identifier(&wolf_sound_variant.adult_sounds.step_sound);
        let baby_ambient_sound = generate_identifier(&wolf_sound_variant.baby_sounds.ambient_sound);
        let baby_death_sound = generate_identifier(&wolf_sound_variant.baby_sounds.death_sound);
        let baby_growl_sound = generate_identifier(&wolf_sound_variant.baby_sounds.growl_sound);
        let baby_hurt_sound = generate_identifier(&wolf_sound_variant.baby_sounds.hurt_sound);
        let baby_pant_sound = generate_identifier(&wolf_sound_variant.baby_sounds.pant_sound);
        let baby_whine_sound = generate_identifier(&wolf_sound_variant.baby_sounds.whine_sound);
        let baby_step_sound = generate_identifier(&wolf_sound_variant.baby_sounds.step_sound);

        stream.extend(quote! {
            pub static #wolf_sound_variant_ident: &WolfSoundVariant = &WolfSoundVariant {
                key: #key,
                adult_sounds: WolfAge {
                    ambient_sound: #adult_ambient_sound,
                    death_sound: #adult_death_sound,
                    growl_sound: #adult_growl_sound,
                    hurt_sound: #adult_hurt_sound,
                    pant_sound: #adult_pant_sound,
                    whine_sound: #adult_whine_sound,
                    step_sound: #adult_step_sound,
                },
                baby_sounds: WolfAge {
                    ambient_sound: #baby_ambient_sound,
                    death_sound: #baby_death_sound,
                    growl_sound: #baby_growl_sound,
                    hurt_sound: #baby_hurt_sound,
                    pant_sound: #baby_pant_sound,
                    whine_sound: #baby_whine_sound,
                    step_sound: #baby_step_sound,
                }
            };
        });

        register_stream.extend(quote! {
            registry.register(#wolf_sound_variant_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_wolf_sound_variants(registry: &mut WolfSoundVariantRegistry) {
            #register_stream
        }
    });

    stream
}
