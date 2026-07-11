use crate::generator_functions::{
    generate_identifier, read_json_asset, sort_contiguous_registry_entries,
};
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize)]
struct SoundEventEntry {
    id: usize,
    key: Identifier,
    sound_id: Identifier,
    #[serde(default)]
    fixed_range: Option<f32>,
}

pub(crate) fn build() -> TokenStream {
    const ASSET: &str = "build_assets/sound_events.json";

    let mut sound_events: Vec<SoundEventEntry> = read_json_asset(ASSET);
    sort_contiguous_registry_entries(&mut sound_events, ASSET, |entry| entry.id);

    let mut constants = TokenStream::new();
    let mut registrations = TokenStream::new();

    for sound_event in &sound_events {
        let ident = Ident::new(
            &sound_event.key.path.to_shouty_snake_case(),
            Span::call_site(),
        );
        let key = generate_identifier(&sound_event.key);
        let sound_id = generate_identifier(&sound_event.sound_id);
        let fixed_range = if let Some(range) = sound_event.fixed_range {
            quote! { Some(#range) }
        } else {
            quote! { None }
        };

        constants.extend(quote! {
            pub static #ident: SoundEvent = SoundEvent {
                key: #key,
                sound_id: #sound_id,
                fixed_range: #fixed_range,
            };
        });

        registrations.extend(quote! {
            registry.register(&#ident);
        });
    }

    quote! {
        //! Sound event registry entries matching vanilla Minecraft's `SoundEvents`.

        use crate::sound_event::{SoundEvent, SoundEventRegistry};
        use std::borrow::Cow;
        use steel_utils::Identifier;

        #constants

        pub fn register_sound_events(registry: &mut SoundEventRegistry) {
            #registrations
        }
    }
}
