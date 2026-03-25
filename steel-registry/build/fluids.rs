use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
struct BehaviorProperties {
    is_empty: bool,
    is_source: bool,
    #[allow(dead_code)]
    is_flowing_fluid: bool,
    #[serde(default)]
    source_fluid: Option<String>,
    #[serde(default)]
    flowing_fluid: Option<String>,
    #[serde(default)]
    tick_delay: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    drop_off: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    slope_find_distance: Option<u32>,
    #[serde(default)]
    explosion_resistance: Option<f32>,
}

#[derive(Deserialize, Debug, Clone)]
struct FluidJson {
    id: u32,
    name: String,
    block: String,
    bucket_item: String,
    behavior_properties: BehaviorProperties,
    #[serde(default)]
    #[allow(dead_code)]
    properties: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    default_properties: Vec<String>,
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/fluids.json");

    let content =
        fs::read_to_string("build_assets/fluids.json").expect("Failed to read fluids.json");
    let fluids: Vec<FluidJson> =
        serde_json::from_str(&content).expect("Failed to parse fluids.json");

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::fluid::{Fluid, FluidRegistry};
        use steel_utils::Identifier;
    });

    // Generate static fluid definitions
    for fluid in &fluids {
        let fluid_ident = Ident::new(&fluid.name.to_shouty_snake_case(), Span::call_site());
        let fluid_name = &fluid.name;
        let is_empty = fluid.behavior_properties.is_empty;
        let is_source = fluid.behavior_properties.is_source;
        let block = &fluid.block;
        let bucket_item = &fluid.bucket_item;
        let tick_delay = fluid.behavior_properties.tick_delay.unwrap_or(0);
        let explosion_resistance = fluid
            .behavior_properties
            .explosion_resistance
            .unwrap_or(0.0);

        let source_fluid = match &fluid.behavior_properties.source_fluid {
            Some(s) => quote! { Some(Identifier::vanilla_static(#s)) },
            None => quote! { None },
        };

        let flowing_fluid = match &fluid.behavior_properties.flowing_fluid {
            Some(s) => quote! { Some(Identifier::vanilla_static(#s)) },
            None => quote! { None },
        };

        stream.extend(quote! {
            pub static #fluid_ident: Fluid = Fluid {
                key: Identifier::vanilla_static(#fluid_name),
                is_empty: #is_empty,
                is_source: #is_source,
                block: Identifier::vanilla_static(#block),
                bucket_item: Identifier::vanilla_static(#bucket_item),
                source_fluid: #source_fluid,
                flowing_fluid: #flowing_fluid,
                tick_delay: #tick_delay,
                explosion_resistance: #explosion_resistance,
            };
        });
    }

    // Generate registration function (order matters - must match vanilla IDs)
    let mut register_stream = TokenStream::new();

    // Sort by ID to ensure correct registration order
    let mut sorted_fluids = fluids.clone();
    sorted_fluids.sort_by_key(|f| f.id);

    for fluid in &sorted_fluids {
        let fluid_ident = Ident::new(&fluid.name.to_shouty_snake_case(), Span::call_site());
        register_stream.extend(quote! {
            registry.register(&#fluid_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_fluids(registry: &mut FluidRegistry) {
            #register_stream
        }
    });

    stream
}
