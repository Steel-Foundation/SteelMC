use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{DeriveInput, Ident, Meta, parse_macro_input};

pub fn client_packet_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    // Find the packet_id attributes
    let attrs: Vec<_> = input
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("packet_id"))
        .collect();

    assert!(
        !attrs.is_empty(),
        "ClientPacket derive macro requires at least one #[packet_id(...)] attribute"
    );

    let mut match_arms = Vec::new();

    for attr in attrs {
        if let Meta::List(meta) = attr.meta.clone() {
            meta.parse_nested_meta(|meta| {
                let state = meta
                    .path
                    .get_ident()
                    .expect("Expected an identifier for the protocol state")
                    .to_string();
                let value: syn::Expr = meta.value()?.parse()?;
                let state_ident = Ident::new(&state, Span::call_site());

                let arm = quote! {
                    crate::utils::ConnectionProtocol::#state_ident => Some(#value),
                };
                match_arms.push(arm);

                Ok(())
            })
            .unwrap_or_else(|e| panic!("Failed to parse `packet_id` attribute: {e}"));
        } else {
            panic!("`packet_id` attribute must be a list: `#[packet_id(STATE = \"path\", ...)]`");
        }
    }

    let (impl_generics, ty_generics, _) = input.generics.split_for_impl();

    let expanded = quote! {
        #[automatically_derived]
        impl #impl_generics crate::packet_traits::ClientPacket for #name #ty_generics {
            fn get_id(&self, protocol: crate::utils::ConnectionProtocol) -> Option<i32> {
                match protocol {
                    #(#match_arms)*
                    _ => None,
                }
            }
        }
    };

    TokenStream::from(expanded)
}

/// Derives the `ServerPacket` trait for a struct.
pub fn server_packet_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    //let attr = input
    //    .attrs
    //    .iter()
    //    .find(|a| a.path().is_ident("packet_id"))
    //    .expect("ServerPacket requires a #[packet_id(...)] attribute");

    //let id_expr: Expr = if let Meta::List(meta) = &attr.meta {
    //    syn::parse2(meta.tokens.clone())
    //        .expect("Failed to parse packet_id content as expression")
    //} else {
    //    panic!("`packet_id` must be used as #[packet_id(...)]");
    //};

    let expanded = quote! {
        #[automatically_derived]
        impl crate::packet_traits::ServerPacket for #name {
    //        const ID: i32 = #id_expr as i32;
        }
    };

    TokenStream::from(expanded)
}
