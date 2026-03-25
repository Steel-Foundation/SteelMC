use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
pub struct Age {
    ambient_sound: Identifier,
    beg_for_food_sound: Identifier,
    death_sound: Identifier,
    eat_sound: Identifier,
    hiss_sound: Identifier,
    hurt_sound: Identifier,
    purr_sound: Identifier,
    purreow_sound: Identifier,
    stray_ambient_sound: Identifier,
}
#[derive(Deserialize, Debug)]
pub struct CatSoundVariantJson {
    adult_sounds: Age,
    baby_sounds: Age,
}

fn generate_identifier(resource: &Identifier) -> TokenStream {
    let path = resource.path.as_ref();
    quote! { Identifier::vanilla_static(#path) }
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/cat_sound_variant/"
    );

    let cat_sound_variant_dir =
        "build_assets/builtin_datapacks/minecraft/data/minecraft/cat_sound_variant";
    let mut cat_sound_variants = Vec::new();

    // Read all cat sound variant JSON files
    for entry in fs::read_dir(cat_sound_variant_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let cat_sound_variant_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let cat_sound_variant: CatSoundVariantJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", cat_sound_variant_name, e));

            cat_sound_variants.push((cat_sound_variant_name, cat_sound_variant));
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::cat_sound_variant::{
            CatSoundVariant, CatSoundVariantRegistry, CatAge
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;
    });

    // Generate static cat sound variant definitions
    let mut register_stream = TokenStream::new();
    for (cat_sound_variant_name, cat_sound_variant) in &cat_sound_variants {
        let cat_sound_variant_ident = Ident::new(
            &cat_sound_variant_name.to_shouty_snake_case(),
            Span::call_site(),
        );
        let cat_sound_variant_name_str = cat_sound_variant_name.clone();

        let key = quote! { Identifier::vanilla_static(#cat_sound_variant_name_str) };
        let adult_ambient_sound =
            generate_identifier(&cat_sound_variant.adult_sounds.ambient_sound);
        let adult_beg_for_food_sound =
            generate_identifier(&cat_sound_variant.adult_sounds.beg_for_food_sound);
        let adult_death_sound = generate_identifier(&cat_sound_variant.adult_sounds.death_sound);
        let adult_eat_sound = generate_identifier(&cat_sound_variant.adult_sounds.eat_sound);
        let adult_hiss_sound = generate_identifier(&cat_sound_variant.adult_sounds.hiss_sound);
        let adult_hurt_sound = generate_identifier(&cat_sound_variant.adult_sounds.hurt_sound);
        let adult_purr_sound = generate_identifier(&cat_sound_variant.adult_sounds.purr_sound);
        let adult_purreow_sound =
            generate_identifier(&cat_sound_variant.adult_sounds.purreow_sound);
        let adult_stray_ambient_sound =
            generate_identifier(&cat_sound_variant.adult_sounds.stray_ambient_sound);
        let baby_ambient_sound = generate_identifier(&cat_sound_variant.baby_sounds.ambient_sound);
        let baby_beg_for_food_sound =
            generate_identifier(&cat_sound_variant.baby_sounds.beg_for_food_sound);
        let baby_death_sound = generate_identifier(&cat_sound_variant.baby_sounds.death_sound);
        let baby_eat_sound = generate_identifier(&cat_sound_variant.baby_sounds.eat_sound);
        let baby_hiss_sound = generate_identifier(&cat_sound_variant.baby_sounds.hiss_sound);
        let baby_hurt_sound = generate_identifier(&cat_sound_variant.baby_sounds.hurt_sound);
        let baby_purr_sound = generate_identifier(&cat_sound_variant.baby_sounds.purr_sound);
        let baby_purreow_sound = generate_identifier(&cat_sound_variant.baby_sounds.purreow_sound);
        let baby_stray_ambient_sound =
            generate_identifier(&cat_sound_variant.baby_sounds.stray_ambient_sound);

        stream.extend(quote! {
            pub static #cat_sound_variant_ident: &CatSoundVariant = &CatSoundVariant {
                key: #key,
                adult_sounds: CatAge {
                    ambient_sound: #adult_ambient_sound,
                    beg_for_food_sound: #adult_beg_for_food_sound,
                    death_sound: #adult_death_sound,
                    eat_sound: #adult_eat_sound,
                    hiss_sound: #adult_hiss_sound,
                    hurt_sound: #adult_hurt_sound,
                    purr_sound: #adult_purr_sound,
                    purreow_sound: #adult_purreow_sound,
                    stray_ambient_sound: #adult_stray_ambient_sound,
                    },
                baby_sounds: CatAge {
                    ambient_sound: #baby_ambient_sound,
                    beg_for_food_sound: #baby_beg_for_food_sound,
                    death_sound: #baby_death_sound,
                    eat_sound: #baby_eat_sound,
                    hiss_sound: #baby_hiss_sound,
                    hurt_sound: #baby_hurt_sound,
                    purr_sound: #baby_purr_sound,
                    purreow_sound: #baby_purreow_sound,
                    stray_ambient_sound: #baby_stray_ambient_sound,
                }
            };
        });

        register_stream.extend(quote! {
            registry.register(#cat_sound_variant_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_cat_sound_variants(registry: &mut CatSoundVariantRegistry) {
            #register_stream
        }
    });

    stream
}
