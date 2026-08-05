use std::collections::BTreeMap;

use crate::generator_functions::{generate_sound_event_ref, read_json_asset};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize)]
struct SoundTypeData {
    volume: f32,
    pitch: f32,
    break_sound: Identifier,
    step_sound: Identifier,
    place_sound: Identifier,
    hit_sound: Identifier,
    fall_sound: Identifier,
}

pub(crate) fn build() -> TokenStream {
    const ASSET: &str = "build_assets/sound_types.json";

    let sound_types: BTreeMap<String, SoundTypeData> = read_json_asset(ASSET);

    let consts: TokenStream = sound_types
        .iter()
        .map(|(name, data)| {
            let name = format_ident!("{}", name);
            let volume = data.volume;
            let pitch = data.pitch;
            let break_sound = generate_sound_event_ref(&data.break_sound);
            let step_sound = generate_sound_event_ref(&data.step_sound);
            let place_sound = generate_sound_event_ref(&data.place_sound);
            let hit_sound = generate_sound_event_ref(&data.hit_sound);
            let fall_sound = generate_sound_event_ref(&data.fall_sound);
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
        //! Sound type definitions matching vanilla Minecraft's `SoundType`.

        use crate::sound_event::SoundEventRef;

        /// Defines the sounds for a block type.
        #[derive(Debug, Clone, Copy)]
        pub struct SoundType {
            /// Volume multiplier for sounds (1.0 = normal).
            pub volume: f32,
            /// Pitch multiplier for sounds (1.0 = normal).
            pub pitch: f32,
            /// Sound event for breaking the block.
            pub break_sound: SoundEventRef,
            /// Sound event for stepping on the block.
            pub step_sound: SoundEventRef,
            /// Sound event for placing the block.
            pub place_sound: SoundEventRef,
            /// Sound event for hitting the block.
            pub hit_sound: SoundEventRef,
            /// Sound event for falling on the block.
            pub fall_sound: SoundEventRef,
        }

        #consts
    )
}
