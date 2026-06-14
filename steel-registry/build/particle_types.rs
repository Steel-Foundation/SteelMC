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

        constants.extend(quote! {
            pub static #ident: ParticleType = ParticleType {
                key: #key,
                override_limiter: #override_limiter,
            };
        });

        registrations.extend(quote! {
            registry.register(&#ident);
        });
    }

    quote! {
        use crate::particle_type::{ParticleType, ParticleTypeRegistry};
        use std::borrow::Cow;
        use steel_utils::Identifier;

        #constants

        pub fn register_particle_types(registry: &mut ParticleTypeRegistry) {
            #registrations
        }
    }
}
