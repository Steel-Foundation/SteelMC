use std::{fs, str::FromStr};

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MapDecorationTypeJson {
    id: usize,
    key: String,
    asset_id: String,
    show_on_item_frame: bool,
    map_color: i32,
    exploration_map_element: bool,
    track_count: bool,
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/map_decoration_types.json");
    let decorations: Vec<MapDecorationTypeJson> = serde_json::from_str(
        &fs::read_to_string("build_assets/map_decoration_types.json")
            .expect("missing extracted map decoration type registry"),
    )
    .expect("invalid extracted map decoration type registry");

    let mut definitions = TokenStream::new();
    let mut registrations = TokenStream::new();
    for (expected_id, decoration) in decorations.iter().enumerate() {
        assert_eq!(
            decoration.id, expected_id,
            "map decoration type registry IDs must be dense"
        );
        let key = Identifier::from_str(&decoration.key)
            .unwrap_or_else(|error| panic!("invalid map decoration type key: {error}"));
        assert_eq!(
            key.namespace.as_ref(),
            "minecraft",
            "extracted Vanilla map decoration type must use the minecraft namespace"
        );
        let asset_id = Identifier::from_str(&decoration.asset_id)
            .unwrap_or_else(|error| panic!("invalid map decoration asset ID: {error}"));
        let ident = Ident::new(&key.path.to_shouty_snake_case(), Span::call_site());
        let key_namespace = key.namespace.as_ref();
        let key_path = key.path.as_ref();
        let asset_namespace = asset_id.namespace.as_ref();
        let asset_path = asset_id.path.as_ref();
        let show_on_item_frame = decoration.show_on_item_frame;
        let map_color = decoration.map_color;
        let exploration_map_element = decoration.exploration_map_element;
        let track_count = decoration.track_count;

        definitions.extend(quote! {
            pub static #ident: MapDecorationType = MapDecorationType::new(
                Identifier::new_static(#key_namespace, #key_path),
                Identifier::new_static(#asset_namespace, #asset_path),
                #show_on_item_frame,
                #map_color,
                #exploration_map_element,
                #track_count,
            );
        });
        registrations.extend(quote! { registry.register(&#ident); });
    }

    quote! {
        use crate::map_decoration_type::{MapDecorationType, MapDecorationTypeRegistry};
        use steel_utils::Identifier;

        #definitions

        pub fn register_map_decoration_types(registry: &mut MapDecorationTypeRegistry) {
            #registrations
        }
    }
}
