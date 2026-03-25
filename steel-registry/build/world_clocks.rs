use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct WorldClockJson {}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/world_clock/"
    );

    let world_clock_dir = "build_assets/builtin_datapacks/minecraft/data/minecraft/world_clock";
    let mut world_clocks = Vec::new();

    // Read all world_clock JSON files
    for entry in fs::read_dir(world_clock_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let world_clock_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let _world_clock: WorldClockJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", world_clock_name, e));

            world_clocks.push(world_clock_name);
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::world_clock::{WorldClock, WorldClockRegistry};
        use steel_utils::Identifier;
    });

    // Generate static world_clock definitions
    let mut register_stream = TokenStream::new();
    for world_clock_name in &world_clocks {
        let world_clock_ident =
            Ident::new(&world_clock_name.to_shouty_snake_case(), Span::call_site());
        let world_clock_name_str = world_clock_name.clone();

        let key = quote! { Identifier::vanilla_static(#world_clock_name_str) };

        stream.extend(quote! {
            pub static #world_clock_ident: &WorldClock = &WorldClock {
                key: #key,
            };
        });

        register_stream.extend(quote! {
            registry.register(#world_clock_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_world_clocks(registry: &mut WorldClockRegistry) {
            #register_stream
        }
    });

    stream
}
