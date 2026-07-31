//! # Steel Macros
//!
//! Macros for the Steel Minecraft server.

mod behavior;
mod packet;
mod read_from;
mod strategy;
mod write_to;

use proc_macro::TokenStream;

/// Marks a struct as a block behavior for auto-registration.
///
/// The build script scans source files for this attribute and generates
/// `register_block_behaviors()` from `classes.json`.
///
/// Use `#[json_arg(...)]` on fields to describe extra constructor arguments.
/// These attributes are stripped before compilation.
#[proc_macro_attribute]
pub fn block_behavior(attr: TokenStream, item: TokenStream) -> TokenStream {
    behavior::block_behavior(attr.into(), item.into()).into()
}

/// Marks a struct as an item behavior for auto-registration.
///
/// The build script scans source files for this attribute and generates
/// `register_item_behaviors()` from `classes.json`.
///
/// Use `#[json_arg(...)]` on fields to describe extra constructor arguments.
/// These attributes are stripped before compilation.
#[proc_macro_attribute]
pub fn item_behavior(attr: TokenStream, item: TokenStream) -> TokenStream {
    behavior::item_behavior(attr.into(), item.into()).into()
}

/// Marks a struct as an entity implementation for auto-registration.
///
/// The build script scans source files for this attribute and generates
/// `register_entity_factories()` from `classes.json`.
///
/// Use `#[json_arg(...)]` on fields to describe extra constructor arguments.
/// These attributes are stripped before compilation.
#[proc_macro_attribute]
pub fn entity_behavior(attr: TokenStream, item: TokenStream) -> TokenStream {
    behavior::entity_behavior(attr.into(), item.into()).into()
}

/// Derives the `ReadFrom` trait for a struct.
///
/// # Panics
/// - If the derive macro is used on a union.
/// - If the derive macro is used on a struct with unnamed fields.
/// - If the `read` attribute is malformed.
/// - If an unknown read strategy is specified.
#[proc_macro_derive(ReadFrom, attributes(read))]
pub fn read_from_derive(input: TokenStream) -> TokenStream {
    read_from::derive(input)
}

/// Derives the `WriteTo` trait for a struct.
///
/// # Panics
/// - If the derive macro is used on a union.
/// - If the derive macro is used on a struct with unnamed fields.
/// - If the `write` attribute is malformed.
/// - If an unknown write strategy is specified.
#[proc_macro_derive(WriteTo, attributes(write))]
pub fn write_to_derive(input: TokenStream) -> TokenStream {
    write_to::derive(input)
}

/// Derives the `ClientPacket` trait for a struct.
///
/// # Panics
/// - If the `packet_id` attribute is missing or malformed.
#[proc_macro_derive(ClientPacket, attributes(packet_id))]
pub fn client_packet_derive(input: TokenStream) -> TokenStream {
    packet::client_packet_derive(input)
}

/// Derives the `ServerPacket` trait for a struct.
#[proc_macro_derive(ServerPacket, attributes(packet_id))]
pub fn server_packet_derive(input: TokenStream) -> TokenStream {
    packet::server_packet_derive(input)
}
