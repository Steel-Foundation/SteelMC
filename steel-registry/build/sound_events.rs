use std::{collections::BTreeMap, fs};

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/sound_events.json");

    let sound_events: BTreeMap<String, i32> =
        serde_json::from_str(&fs::read_to_string("build_assets/sound_events.json").unwrap())
            .expect("Failed to parse sound_events.json");

    let consts: TokenStream = sound_events
        .iter()
        .map(|(name, value)| {
            let name = format_ident!("{}", name);
            quote! {
                pub const #name: i32 = #value;
            }
        })
        .collect();

    quote!(
        //! Sound event registry IDs matching vanilla Minecraft's SoundEvents.java.
        //!
        //! These IDs are used with `CSound` to play sounds on the client.
        //!
        //! Sound events are organized by category:
        //! - `BLOCK_*` - Block-related sounds (break, place, step, hit, fall)
        //! - `ENTITY_*` - Entity-related sounds
        //! - `ITEM_*` - Item-related sounds
        //! - `AMBIENT_*` - Ambient sounds
        //! - `MUSIC_*` - Music tracks

        #consts
    )
}
