use crate::generator_functions::{
    generate_identifier, generate_option, read_json_asset, sort_contiguous_registry_entries,
};
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use std::collections::BTreeMap;
use steel_utils::Identifier;

#[derive(Deserialize)]
struct VillagerProfessionEntry {
    id: usize,
    key: Identifier,
    work_sound: Option<Identifier>,
}

pub(crate) fn build() -> TokenStream {
    const ASSET: &str = "build_assets/villager_professions.json";

    let mut villager_professions: Vec<VillagerProfessionEntry> = read_json_asset(ASSET);
    sort_contiguous_registry_entries(&mut villager_professions, ASSET, |entry| entry.id);
    let sound_events: BTreeMap<String, i32> = read_json_asset("build_assets/sound_events.json");

    let mut constants = TokenStream::new();
    let mut registrations = TokenStream::new();

    for villager_profession in &villager_professions {
        let ident = Ident::new(
            &villager_profession.key.path.to_shouty_snake_case(),
            Span::call_site(),
        );
        let key = generate_identifier(&villager_profession.key);
        if let Some(work_sound) = &villager_profession.work_sound {
            let sound_event_const = work_sound.path.replace('.', "_").to_shouty_snake_case();
            assert!(
                sound_events.contains_key(&sound_event_const),
                "Villager profession {} references missing work sound {}",
                villager_profession.key,
                work_sound
            );
        }
        let work_sound = generate_option(&villager_profession.work_sound, generate_identifier);

        constants.extend(quote! {
            pub static #ident: VillagerProfession = VillagerProfession {
                key: #key,
                work_sound: #work_sound,
            };
        });

        registrations.extend(quote! {
            registry.register(&#ident);
        });
    }

    quote! {
        use crate::villager_profession::{VillagerProfession, VillagerProfessionRegistry};
        use std::borrow::Cow;
        use steel_utils::Identifier;

        #constants

        pub fn register_villager_professions(registry: &mut VillagerProfessionRegistry) {
            #registrations
        }
    }
}
