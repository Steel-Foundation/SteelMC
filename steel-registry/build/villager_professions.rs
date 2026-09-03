use crate::generator_functions::{
    generate_identifier, generate_option, generate_sound_event_ref, read_json_asset,
    sort_contiguous_registry_entries,
};
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use std::collections::BTreeSet;
use steel_utils::Identifier;

#[derive(Deserialize)]
struct VillagerProfessionEntry {
    id: usize,
    key: Identifier,
    work_sound: Option<Identifier>,
    held_job_site: Option<Vec<Identifier>>,
    acquirable_job_site: Option<Vec<Identifier>>,
}

#[derive(Deserialize)]
struct SoundEventEntry {
    key: Identifier,
}

#[derive(Deserialize)]
struct PoiTypeKeysFile {
    poi_types: Vec<PoiTypeNameEntry>,
}

#[derive(Deserialize)]
struct PoiTypeNameEntry {
    name: String,
}

/// Resolves job-site POI type keys to references of the generated
/// `vanilla_poi_types` constants, validating every key against the extracted POI
/// type registry.
fn generate_job_sites(
    sites: &Option<Vec<Identifier>>,
    poi_type_keys: &BTreeSet<String>,
    profession: &Identifier,
    field: &str,
) -> TokenStream {
    let Some(sites) = sites else {
        return quote! { &[] };
    };

    let refs = sites.iter().map(|key| {
        assert!(
            key.namespace.as_ref() == "minecraft",
            "Villager profession {profession} {field} references non-vanilla POI type {key}"
        );
        assert!(
            poi_type_keys.contains(&key.to_string()),
            "Villager profession {profession} {field} references missing POI type {key}"
        );
        let ident = Ident::new(&key.path.to_shouty_snake_case(), Span::call_site());
        quote! { &crate::vanilla_poi_types::#ident }
    });

    quote! { &[#(#refs),*] }
}

pub(crate) fn build() -> TokenStream {
    const ASSET: &str = "build_assets/villager_professions.json";

    let mut villager_professions: Vec<VillagerProfessionEntry> = read_json_asset(ASSET);
    sort_contiguous_registry_entries(&mut villager_professions, ASSET, |entry| entry.id);
    let sound_events: Vec<SoundEventEntry> = read_json_asset("build_assets/sound_events.json");
    let sound_event_keys: BTreeSet<String> = sound_events
        .into_iter()
        .map(|entry| entry.key.to_string())
        .collect();
    let poi_file: PoiTypeKeysFile = read_json_asset("build_assets/poi_types.json");
    let poi_type_keys: BTreeSet<String> = poi_file
        .poi_types
        .into_iter()
        .map(|entry| format!("minecraft:{}", entry.name))
        .collect();

    let mut constants = TokenStream::new();
    let mut registrations = TokenStream::new();

    for villager_profession in &villager_professions {
        let ident = Ident::new(
            &villager_profession.key.path.to_shouty_snake_case(),
            Span::call_site(),
        );
        let key = generate_identifier(&villager_profession.key);
        if let Some(work_sound) = &villager_profession.work_sound {
            assert!(
                sound_event_keys.contains(&work_sound.to_string()),
                "Villager profession {} references missing work sound {}",
                villager_profession.key,
                work_sound
            );
        }
        let work_sound = generate_option(&villager_profession.work_sound, generate_sound_event_ref);
        let held_job_site = generate_job_sites(
            &villager_profession.held_job_site,
            &poi_type_keys,
            &villager_profession.key,
            "held_job_site",
        );
        let acquirable_job_site = generate_job_sites(
            &villager_profession.acquirable_job_site,
            &poi_type_keys,
            &villager_profession.key,
            "acquirable_job_site",
        );

        constants.extend(quote! {
            pub static #ident: VillagerProfession = VillagerProfession {
                key: #key,
                work_sound: #work_sound,
                held_job_site: #held_job_site,
                acquirable_job_site: #acquirable_job_site,
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
