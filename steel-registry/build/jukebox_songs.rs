use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
pub struct JukeboxSongJson {
    sound_event: Identifier,
    description: TextComponentJson,
    length_in_seconds: f32,
    comparator_output: i32,
}

#[derive(Deserialize, Debug)]
pub struct TextComponentJson {
    translate: String,
}

fn generate_identifier(resource: &Identifier) -> TokenStream {
    let namespace = resource.namespace.as_ref();
    let path = resource.path.as_ref();
    quote! { Identifier { namespace: Cow::Borrowed(#namespace), path: Cow::Borrowed(#path) } }
}

fn generate_text_component(component: &TextComponentJson) -> TokenStream {
    let translate = component.translate.as_str();
    quote! {
        TextComponent::const_translate(#translate)
    }
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/jukebox_song/"
    );

    let jukebox_song_dir = "build_assets/builtin_datapacks/minecraft/data/minecraft/jukebox_song";
    let mut jukebox_songs = Vec::new();

    // Read all jukebox song JSON files
    for entry in fs::read_dir(jukebox_song_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let jukebox_song_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let jukebox_song: JukeboxSongJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", jukebox_song_name, e));

            jukebox_songs.push((jukebox_song_name, jukebox_song));
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::jukebox_song::{
            JukeboxSong, JukeboxSongRegistry,
        };
        use steel_utils::Identifier;
        use steel_utils::text::TextComponent;
        use std::borrow::Cow;
    });

    // Generate static jukebox song definitions
    for (jukebox_song_name, jukebox_song) in &jukebox_songs {
        // Handle special case where song name is a number (e.g., "13" -> "MUSIC_DISC_13")
        let jukebox_song_ident = if jukebox_song_name.chars().next().unwrap().is_ascii_digit() {
            Ident::new(
                &format!("MUSIC_DISC_{}", jukebox_song_name.to_shouty_snake_case()),
                Span::call_site(),
            )
        } else {
            Ident::new(&jukebox_song_name.to_shouty_snake_case(), Span::call_site())
        };
        let jukebox_song_name_str = jukebox_song_name.clone();

        let key = quote! { Identifier::vanilla_static(#jukebox_song_name_str) };
        let sound_event = generate_identifier(&jukebox_song.sound_event);
        let description = generate_text_component(&jukebox_song.description);
        let length_in_seconds = jukebox_song.length_in_seconds;
        let comparator_output = jukebox_song.comparator_output;

        stream.extend(quote! {
            pub static #jukebox_song_ident: &JukeboxSong = &JukeboxSong {
                key: #key,
                sound_event: #sound_event,
                description: #description,
                length_in_seconds: #length_in_seconds,
                comparator_output: #comparator_output,
            };
        });
    }

    // Generate registration function
    let mut register_stream = TokenStream::new();
    for (jukebox_song_name, _) in &jukebox_songs {
        let jukebox_song_ident = if jukebox_song_name.chars().next().unwrap().is_ascii_digit() {
            Ident::new(
                &format!("MUSIC_DISC_{}", jukebox_song_name.to_shouty_snake_case()),
                Span::call_site(),
            )
        } else {
            Ident::new(&jukebox_song_name.to_shouty_snake_case(), Span::call_site())
        };
        register_stream.extend(quote! {
            registry.register(#jukebox_song_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_jukebox_songs(registry: &mut JukeboxSongRegistry) {
            #register_stream
        }
    });

    stream
}
