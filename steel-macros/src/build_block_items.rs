use proc_macro2::TokenStream;
use quote::quote;
use syn::{Fields, ItemStruct, parse2};

/// Attribute macro for block behavior structs.
///
/// Strips `#[json_arg(...)]` field attributes (which are only read by the build script
/// scanning source files) and passes the struct through unchanged.
pub fn block_behavior(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input: ItemStruct =
        parse2(item).expect("#[block_behavior] can only be applied to structs");

    if let Fields::Named(ref mut fields) = input.fields {
        for field in &mut fields.named {
            field.attrs.retain(|attr| !attr.path().is_ident("json_arg"));
        }
    }

    quote! { #input }
}
