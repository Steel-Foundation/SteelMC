#![expect(
    clippy::unwrap_used,
    reason = "build script must fail immediately on invalid extracted packet data"
)]

use rustc_hash::FxHashMap;
use std::{collections::BTreeMap, fs};

use heck::ToSnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Packets {
    version: i32,
    serverbound: BTreeMap<String, Vec<String>>,
    clientbound: BTreeMap<String, Vec<String>>,
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/packets.json");

    let packets: Packets =
        serde_json::from_str(&fs::read_to_string("build_assets/packets.json").unwrap())
            .expect("Failed to parse packets.json");

    let mut phases: FxHashMap<String, TokenStream> = FxHashMap::default();

    let version = packets.version;
    parse_packets(
        packets.clientbound,
        Ident::new("C", Span::call_site()),
        &mut phases,
    );
    parse_packets(
        packets.serverbound,
        Ident::new("S", Span::call_site()),
        &mut phases,
    );

    let consts: TokenStream = phases
        .iter()
        .map(|(p, entries)| {
            let phase = Ident::new(p, Span::call_site());
            quote! {
                pub mod #phase {
                    #entries
                }
            }
        })
        .collect();

    quote!(
        /// The current Minecraft protocol version. This changes only when the protocol itself is modified.
        pub const CURRENT_MC_PROTOCOL: i32 = #version;

        #consts
    )
}

pub(crate) fn parse_packets(
    packets: BTreeMap<String, Vec<String>>,
    prefix: Ident,
    phases: &mut FxHashMap<String, TokenStream>,
) {
    for packet in packets {
        let inner = phases.entry(packet.0.to_snake_case()).or_default();

        for (id, packet_name) in packet.1.iter().enumerate() {
            let packet_id = id as i32;
            let packet_name = packet_name.replace('/', "_");
            let name = format!("{prefix}_{packet_name}").to_uppercase();
            let name = format_ident!("{}", name);
            inner.extend([quote! {
                pub const #name: i32 = #packet_id;
            }]);
        }
    }
}
