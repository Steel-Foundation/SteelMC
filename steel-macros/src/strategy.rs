/// Represents a parsed strategy from read/write attributes.
///
/// Supports:
/// - Simple: `VarInt`, `Byte`, `Json`
/// - Container: `Prefixed(VarInt)`, `Prefixed(VarInt, inner = VarInt)`
/// - Unprefixed: `Unprefixed`, `Unprefixed(inner = VarInt)`
#[derive(Clone)]
pub(super) struct Strategy {
    pub(super) name: Ident,
    /// For Prefixed: the prefix type (e.g., `VarInt`, u16)
    pub(super) prefix_type: Option<syn::Type>,
    /// For container strategies: how to read/write inner elements
    pub(super) inner: Option<Box<Strategy>>,
}

impl Strategy {
    pub(super) fn name_str(&self) -> String {
        self.name.to_string()
    }

    /// Gets the prefix type as a token stream, expanding known identifiers to full paths.
    pub(super) fn prefix_type_tokens(&self) -> Option<TokenStream> {
        self.prefix_type.as_ref().map(expand_known_type)
    }
}

/// Expands known type identifiers to their fully qualified paths.
///
/// For example, `VarInt` becomes `steel_utils::codec::VarInt`.
pub(super) fn expand_known_type(ty: &syn::Type) -> TokenStream {
    // Check if it's a simple path (single identifier)
    if let syn::Type::Path(type_path) = ty
        && type_path.qself.is_none()
        && type_path.path.segments.len() == 1
    {
        let segment = &type_path.path.segments[0];
        if segment.arguments.is_empty() {
            let ident_str = segment.ident.to_string();
            // Expand known codec types
            match ident_str.as_str() {
                "VarInt" => return quote! { steel_utils::codec::VarInt },
                "VarLong" => return quote! { steel_utils::codec::VarLong },
                _ => {}
            }
        }
    }
    // For unknown types, use as-is
    quote! { #ty }
}

impl Parse for Strategy {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;

        let mut prefix_type = None;
        let mut inner = None;

        // Check for parentheses with arguments
        if input.peek(Paren) {
            let content;
            syn::parenthesized!(content in input);

            if !content.is_empty() {
                // Check if first token is "inner" (for Unprefixed(inner = ...))
                let is_inner_first = {
                    let fork = content.fork();
                    if let Ok(ident) = fork.parse::<Ident>() {
                        ident == "inner" && fork.peek(syn::Token![=])
                    } else {
                        false
                    }
                };

                if is_inner_first {
                    // Parse: inner = Strategy
                    content.parse::<Ident>()?; // consume "inner"
                    content.parse::<syn::Token![=]>()?;
                    inner = Some(Box::new(content.parse()?));
                } else {
                    // First argument is prefix type
                    prefix_type = Some(content.parse()?);

                    // Check for ", inner = ..."
                    if content.peek(syn::Token![,]) {
                        content.parse::<syn::Token![,]>()?;

                        if !content.is_empty() {
                            let inner_kw: Ident = content.parse()?;
                            if inner_kw != "inner" {
                                return Err(syn::Error::new(inner_kw.span(), "expected `inner`"));
                            }
                            content.parse::<syn::Token![=]>()?;
                            inner = Some(Box::new(content.parse()?));
                        }
                    }
                }
            }
        }

        Ok(Strategy {
            name,
            prefix_type,
            inner,
        })
    }
}
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Ident,
    parse::{Parse, ParseStream},
    token::Paren,
};

pub(super) const ALLOWED_TYPES: [&str; 12] = [
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
];
