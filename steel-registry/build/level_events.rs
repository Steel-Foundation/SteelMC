use std::{collections::BTreeMap, fs};

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/level_events.json");

    let level_events: BTreeMap<String, i32> =
        serde_json::from_str(&fs::read_to_string("build_assets/level_events.json").unwrap())
            .expect("Failed to parse level_events.json");

    let consts: TokenStream = level_events
        .iter()
        .map(|(name, value)| {
            let name = format_ident!("{}", name);
            quote! {
                pub const #name: i32 = #value;
            }
        })
        .collect();

    quote!(
        //! Level event constants matching vanilla Minecraft's LevelEvent.java.
        //!
        //! These events are sent via `CLevelEvent` to trigger sound, particle, and animation
        //! effects on the client side.

        #consts

        /// Helper to encode a block state ID for use with [`PARTICLES_DESTROY_BLOCK`].
        #[inline]
        pub const fn encode_block_state_data(block_state_id: u32) -> i32 {
            block_state_id as i32
        }

        /// Smoke direction constants for use with [`PARTICLES_SHOOT_SMOKE`].
        pub mod smoke_direction {
            pub const DOWN: i32 = 0;
            pub const UP: i32 = 1;
            pub const NORTH: i32 = 2;
            pub const SOUTH: i32 = 3;
            pub const WEST: i32 = 4;
            pub const EAST: i32 = 5;
        }
    )
}
