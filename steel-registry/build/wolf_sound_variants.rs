use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
pub struct WolfSoundVariantJson {
    ambient_sound: Identifier,
    death_sound: Identifier,
    growl_sound: Identifier,
    hurt_sound: Identifier,
    pant_sound: Identifier,
    whine_sound: Identifier,
}

fn generate_identifier(resource: &Identifier) -> TokenStream {
    let namespace = resource.namespace.as_ref();
    let path = resource.path.as_ref();
    quote! { Identifier { namespace: Cow::Borrowed(#namespace), path: Cow::Borrowed(#path) } }
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/wolf_sound_variant/"
    );

    let wolf_sound_variant_dir =
        "build_assets/builtin_datapacks/minecraft/data/minecraft/wolf_sound_variant";
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
            WolfSoundVariant, WolfSoundVariantRegistry,
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
        let ambient_sound = generate_identifier(&wolf_sound_variant.ambient_sound);
        let death_sound = generate_identifier(&wolf_sound_variant.death_sound);
        let growl_sound = generate_identifier(&wolf_sound_variant.growl_sound);
        let hurt_sound = generate_identifier(&wolf_sound_variant.hurt_sound);
        let pant_sound = generate_identifier(&wolf_sound_variant.pant_sound);
        let whine_sound = generate_identifier(&wolf_sound_variant.whine_sound);

        stream.extend(quote! {
            pub static #wolf_sound_variant_ident: &WolfSoundVariant = &WolfSoundVariant {
                key: #key,
                ambient_sound: #ambient_sound,
                death_sound: #death_sound,
                growl_sound: #growl_sound,
                hurt_sound: #hurt_sound,
                pant_sound: #pant_sound,
                whine_sound: #whine_sound,
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
