use proc_macro2::TokenStream;
use quote::quote;
use syn::{Fields, ItemStruct, parse2};

/// Attribute macro for block behavior structs.
///
/// Strips `#[json_arg(...)]` field attributes (which are only read by the build script
/// scanning source files) and passes the struct through unchanged.
pub fn block_behavior(_attr: TokenStream, item: TokenStream) -> TokenStream {
    strip_json_arg_attrs(item, "block_behavior")
}

/// Attribute macro for item behavior structs.
///
/// Strips `#[json_arg(...)]` field attributes (which are only read by the build script
/// scanning source files) and passes the struct through unchanged.
pub fn item_behavior(_attr: TokenStream, item: TokenStream) -> TokenStream {
    strip_json_arg_attrs(item, "item_behavior")
}

fn strip_json_arg_attrs(item: TokenStream, macro_name: &str) -> TokenStream {
    let mut input: ItemStruct =
        parse2(item).unwrap_or_else(|_| panic!("#[{macro_name}] can only be applied to structs"));

    if let Fields::Named(ref mut fields) = input.fields {
        for field in &mut fields.named {
            field.attrs.retain(|attr| !attr.path().is_ident("json_arg"));
        }
    }

    quote! { #input }
}
