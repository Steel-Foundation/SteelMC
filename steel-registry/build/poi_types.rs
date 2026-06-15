use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
struct BlockStateEntry {
    #[expect(dead_code)]
    block: String,
    state_id: u32,
}

#[derive(Deserialize, Debug, Clone)]
struct PoiTypeJson {
    id: u32,
    name: String,
    ticket_count: u32,
    valid_range: u32,
    block_states: Vec<BlockStateEntry>,
}

#[derive(Deserialize, Debug)]
struct PoiTypesFile {
    poi_types: Vec<PoiTypeJson>,
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/poi_types.json");

    let content =
        fs::read_to_string("build_assets/poi_types.json").expect("Failed to read poi_types.json");
    let file: PoiTypesFile =
        serde_json::from_str(&content).expect("Failed to parse poi_types.json");
    let poi_types = file.poi_types;

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::poi::{PointOfInterestType, PoiTypeRegistry};
        use steel_utils::{BlockStateId, Identifier};
    });

    for poi_type in &poi_types {
        let poi_ident = Ident::new(&poi_type.name.to_shouty_snake_case(), Span::call_site());
        let poi_name = &poi_type.name;
        let ticket_count = poi_type.ticket_count;
        let search_distance = poi_type.valid_range;

        let state_ids: Vec<Literal> = poi_type
            .block_states
            .iter()
            .map(|s| Literal::u16_unsuffixed(s.state_id as u16))
            .collect();

        stream.extend(quote! {
            pub static #poi_ident: PointOfInterestType = PointOfInterestType {
                key: Identifier::vanilla_static(#poi_name),
                block_state_ids: &[#(BlockStateId(#state_ids)),*],
                ticket_count: #ticket_count,
                search_distance: #search_distance,
            };
        });
    }

    // Generate registration function (order matters - must match vanilla IDs)
    let mut register_stream = TokenStream::new();

    let mut sorted_poi_types = poi_types.clone();
    sorted_poi_types.sort_by_key(|p| p.id);

    for poi_type in &sorted_poi_types {
        let poi_ident = Ident::new(&poi_type.name.to_shouty_snake_case(), Span::call_site());
        register_stream.extend(quote! {
            registry.register(&#poi_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_poi_types(registry: &mut PoiTypeRegistry) {
            #register_stream
        }
    });

    stream
}
