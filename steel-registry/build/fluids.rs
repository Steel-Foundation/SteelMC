use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
struct FluidJson {
    id: u32,
    name: String,
    is_empty: bool,
    is_source: bool,
    block: String,
    bucket_item: String,
    #[serde(default)]
    source_fluid: Option<String>,
    #[serde(default)]
    flowing_fluid: Option<String>,
    #[serde(default)]
    tick_delay: Option<u32>,
    #[serde(default)]
    explosion_resistance: Option<f32>,
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
        let is_empty = fluid.is_empty;
        let is_source = fluid.is_source;
        let block = &fluid.block;
        let bucket_item = &fluid.bucket_item;
        let tick_delay = fluid.tick_delay.unwrap_or(0);
        let explosion_resistance = fluid.explosion_resistance.unwrap_or(0.0);

        let source_fluid = match &fluid.source_fluid {
            Some(s) => quote! { Some(Identifier::vanilla_static(#s)) },
            None => quote! { None },
        };

        let flowing_fluid = match &fluid.flowing_fluid {
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
