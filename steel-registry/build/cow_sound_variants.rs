use crate::generator_functions::{generate_identifier, read_variants_from_dir};
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

pub(crate) fn build() -> TokenStream {
    let cow_sound_variants: Vec<(String, CowSoundVariantJson)> =
        read_variants_from_dir("cow_sound_variant");

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
            pub static #cow_sound_variant_ident: CowSoundVariant = CowSoundVariant {
                key: #key,
                ambient_sound: #ambient_sound,
                death_sound: #death_sound,
                hurt_sound: #hurt_sound,
                step_sound: #step_sound,
            };
        });

        register_stream.extend(quote! {
            registry.register(&#cow_sound_variant_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_cow_sound_variants(registry: &mut CowSoundVariantRegistry) {
            #register_stream
        }
    });

    stream
}
