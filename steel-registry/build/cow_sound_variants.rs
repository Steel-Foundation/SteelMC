use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
pub struct CowSoundVariantJson {
    ambient_sound: Identifier,
    death_sound: Identifier,
    hurt_sound: Identifier,
    step_sound: Identifier,
}

fn generate_identifier(resource: &Identifier) -> TokenStream {
    let path = resource.path.as_ref();
    quote! { Identifier::vanilla_static(#path) }
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/cow_sound_variant/");

    let cow_sound_variant_dir = "build_assets/builtin_datapacks/minecraft/cow_sound_variant";
    let mut cow_sound_variants = Vec::new();

    // Read all cow sound variant JSON files
    for entry in fs::read_dir(cow_sound_variant_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let cow_sound_variant_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let cow_sound_variant: CowSoundVariantJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", cow_sound_variant_name, e));

            cow_sound_variants.push((cow_sound_variant_name, cow_sound_variant));
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::cow_sound_variant::{
            CowSoundVariant, CowSoundVariantRegistry,
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;
    });

    // Generate static cow sound variant definitions
    let mut register_stream = TokenStream::new();
    for (cow_sound_variant_name, cow_sound_variant) in &cow_sound_variants {
        let cow_sound_variant_ident = Ident::new(
            &cow_sound_variant_name.to_shouty_snake_case(),
            Span::call_site(),
        );
        let cow_sound_variant_name_str = cow_sound_variant_name.clone();

        let key = quote! { Identifier::vanilla_static(#cow_sound_variant_name_str) };
        let ambient_sound = generate_identifier(&cow_sound_variant.ambient_sound);
        let death_sound = generate_identifier(&cow_sound_variant.death_sound);
        let hurt_sound = generate_identifier(&cow_sound_variant.hurt_sound);
        let step_sound = generate_identifier(&cow_sound_variant.step_sound);

        stream.extend(quote! {
            pub static #cow_sound_variant_ident: &CowSoundVariant = &CowSoundVariant {
                key: #key,
                ambient_sound: #ambient_sound,
                death_sound: #death_sound,
                hurt_sound: #hurt_sound,
                step_sound: #step_sound,
            };
        });

        register_stream.extend(quote! {
            registry.register(#cow_sound_variant_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_cow_sound_variants(registry: &mut CowSoundVariantRegistry) {
            #register_stream
        }
    });

    stream
}
