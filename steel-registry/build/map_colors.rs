use std::fs;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Colors {
    colors: Vec<Color>,
    brightness: Vec<Brightness>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Color {
    name: String,
    id: u8,
    color: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Brightness {
    name: String,
    id: u8,
    modifier: u8,
}

pub(crate) fn build() -> TokenStream {
    let path = "build_assets/map_colors.json";
    println!("cargo:rerun-if-changed={path}");
    let colors: Colors = serde_json::from_str(
        &fs::read_to_string(path).unwrap_or_else(|error| panic!("{path}: {error}")),
    )
    .unwrap_or_else(|error| panic!("{path}: {error}"));
    let definitions = colors.colors.iter().map(|color| {
        assert!(
            color.id < 64 && color.color <= 0x00ff_ffff,
            "Invalid map color {}",
            color.name
        );
        let name = format_ident!("{}", color.name);
        let id = color.id;
        let rgb = color.color;
        quote! { pub const #name: MapColor = MapColor { id: #id, color: #rgb }; }
    });
    let brightness = colors.brightness.iter().map(|brightness| {
        assert!(
            brightness.id < 4,
            "Invalid map brightness {}",
            brightness.name
        );
        let name = format_ident!("{}", brightness.name);
        let id = brightness.id;
        let modifier = brightness.modifier;
        quote! { pub const #name: MapBrightness = MapBrightness { id: #id, modifier: #modifier }; }
    });
    quote! {
        use crate::map_color::MapColor;
        #(#definitions)*
        pub mod brightness {
            use crate::map_color::MapBrightness;
            #(#brightness)*
        }
    }
}
