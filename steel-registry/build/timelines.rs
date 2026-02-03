use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct TimelineJson {
    #[serde(default)]
    period_ticks: Option<u32>,
    #[serde(default)]
    tracks: serde_json::Value,
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/timeline/"
    );

    let timeline_dir = "build_assets/builtin_datapacks/minecraft/data/minecraft/timeline";
    let mut timelines = Vec::new();

    // Read all timeline JSON files
    for entry in fs::read_dir(timeline_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let timeline_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let _timeline: TimelineJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", timeline_name, e));

            timelines.push(timeline_name);
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::timeline::{Timeline, TimelineRegistry};
        use steel_utils::Identifier;
        use std::borrow::Cow;
    });

    // Generate static timeline definitions
    let mut register_stream = TokenStream::new();
    for timeline_name in &timelines {
        let timeline_ident = Ident::new(&timeline_name.to_shouty_snake_case(), Span::call_site());
        let timeline_name_str = timeline_name.clone();

        let key = quote! { Identifier::vanilla_static(#timeline_name_str) };

        stream.extend(quote! {
            pub static #timeline_ident: &Timeline = &Timeline {
                key: #key,
            };
        });

        register_stream.extend(quote! {
            registry.register(#timeline_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_timelines(registry: &mut TimelineRegistry) {
            #register_stream
        }
    });

    stream
}
