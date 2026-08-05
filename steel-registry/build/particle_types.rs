use crate::generator_functions::{
    generate_identifier, read_json_asset, sort_contiguous_registry_entries,
};
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize)]
struct ParticleTypeEntry {
    id: usize,
    key: Identifier,
    override_limiter: bool,
    options_type: ParticleOptionsType,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ParticleOptionsType {
    Simple,
    Block,
    Color,
    Dust,
    DustColorTransition,
    Geyser,
    GeyserBase,
    Power,
    Spell,
    Item,
    SculkCharge,
    Shriek,
    Trail,
    Vibration,
}

pub(crate) fn build() -> TokenStream {
    const ASSET: &str = "build_assets/particle_types.json";

    let mut particle_types: Vec<ParticleTypeEntry> = read_json_asset(ASSET);
    sort_contiguous_registry_entries(&mut particle_types, ASSET, |entry| entry.id);

    let mut constants = TokenStream::new();
    let mut registrations = TokenStream::new();

    for particle_type in &particle_types {
        let ident = Ident::new(
            &particle_type.key.path.to_shouty_snake_case(),
            Span::call_site(),
        );
        let key = generate_identifier(&particle_type.key);
        let override_limiter = particle_type.override_limiter;
        let options_type = match particle_type.options_type {
            ParticleOptionsType::Simple => quote! { SimpleParticleOptions },
            ParticleOptionsType::Block => quote! { BlockParticleOption },
            ParticleOptionsType::Color => quote! { ColorParticleOption },
            ParticleOptionsType::Dust => quote! { DustParticleOptions },
            ParticleOptionsType::DustColorTransition => quote! { DustColorTransitionOptions },
            ParticleOptionsType::Geyser => quote! { GeyserParticleOptions },
            ParticleOptionsType::GeyserBase => quote! { GeyserBaseParticleOptions },
            ParticleOptionsType::Power => quote! { PowerParticleOption },
            ParticleOptionsType::Spell => quote! { SpellParticleOption },
            ParticleOptionsType::Item => quote! { ItemParticleOption },
            ParticleOptionsType::SculkCharge => quote! { SculkChargeParticleOptions },
            ParticleOptionsType::Shriek => quote! { ShriekParticleOption },
            ParticleOptionsType::Trail => quote! { TrailParticleOption },
            ParticleOptionsType::Vibration => quote! { VibrationParticleOption },
        };

        constants.extend(quote! {
            pub static #ident: ParticleType =
                ParticleType::of::<#options_type>(#key, #override_limiter);
        });

        registrations.extend(quote! {
            registry.register(&#ident);
        });
    }

    quote! {
        use crate::particle_type::{
            BlockParticleOption, ColorParticleOption, DustColorTransitionOptions,
            DustParticleOptions, GeyserBaseParticleOptions, GeyserParticleOptions,
            ItemParticleOption, ParticleType, ParticleTypeRegistry, PowerParticleOption,
            SculkChargeParticleOptions, ShriekParticleOption, SimpleParticleOptions,
            SpellParticleOption, TrailParticleOption, VibrationParticleOption,
        };
        use std::borrow::Cow;
        use steel_utils::Identifier;

        #constants

        pub fn register_particle_types(registry: &mut ParticleTypeRegistry) {
            #registrations
        }
    }
}
