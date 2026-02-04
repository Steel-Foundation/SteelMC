use std::{collections::BTreeMap, fs};

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde::Deserialize;

#[derive(Deserialize)]
struct SoundTypeData {
    volume: f32,
    pitch: f32,
    break_sound: i32,
    step_sound: i32,
    place_sound: i32,
    hit_sound: i32,
    fall_sound: i32,
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/sound_types.json");

    let sound_types: BTreeMap<String, SoundTypeData> =
        serde_json::from_str(&fs::read_to_string("build_assets/sound_types.json").unwrap())
            .expect("Failed to parse sound_types.json");

    let consts: TokenStream = sound_types
        .iter()
        .map(|(name, data)| {
            let name = format_ident!("{}", name);
            let volume = data.volume;
            let pitch = data.pitch;
            let break_sound = data.break_sound;
            let step_sound = data.step_sound;
            let place_sound = data.place_sound;
            let hit_sound = data.hit_sound;
            let fall_sound = data.fall_sound;
            quote! {
                pub const #name: SoundType = SoundType {
                    volume: #volume,
                    pitch: #pitch,
                    break_sound: #break_sound,
                    step_sound: #step_sound,
                    place_sound: #place_sound,
                    hit_sound: #hit_sound,
                    fall_sound: #fall_sound,
                };
            }
        })
        .collect();

    quote!(
        //! Sound type definitions matching vanilla Minecraft's SoundType.java.
        //!
        //! Each block has a sound type that defines the sounds played when:
        //! - Breaking the block
        //! - Stepping on the block
        //! - Placing the block
        //! - Hitting the block
        //! - Falling on the block
        //!
        //! The sound IDs reference entries in `sound_events`.

        /// Defines the sounds for a block type.
        #[derive(Debug, Clone, Copy)]
        pub struct SoundType {
            /// Volume multiplier for sounds (1.0 = normal).
            pub volume: f32,
            /// Pitch multiplier for sounds (1.0 = normal).
            pub pitch: f32,
            /// Sound event ID for breaking the block.
            pub break_sound: i32,
            /// Sound event ID for stepping on the block.
            pub step_sound: i32,
            /// Sound event ID for placing the block.
            pub place_sound: i32,
            /// Sound event ID for hitting the block.
            pub hit_sound: i32,
            /// Sound event ID for falling on the block.
            pub fall_sound: i32,
        }

        #consts
    )
}
