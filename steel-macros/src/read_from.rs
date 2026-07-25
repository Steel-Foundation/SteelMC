use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Ident, Meta, parse_macro_input};

use crate::strategy::{ALLOWED_TYPES, Strategy};

const UNSUPPORTED_READ_PROP: &str =
    "unsupported property. Supported properties are `as = ...`, `bound = ...`";
const WRONG_READ_FORMAT: &str = "attribute requires a list format: `#[read(as = ..., bound = ..)]";

pub(super) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    match input.data {
        Data::Struct(value) => read_from_struct(value, name, &input.attrs),
        Data::Enum(value) => read_from_enum(value, name, input.attrs),
        Data::Union(_) => panic!("Read can only be derived for structs or enums"),
    }
}

struct FieldReadAttributes {
    strategy: Option<Strategy>,
    bound: Option<syn::LitInt>,
}

fn parse_read_attributes(f: &syn::Field) -> FieldReadAttributes {
    let mut strategy: Option<Strategy> = None;
    let mut bound: Option<syn::LitInt> = None;

    if let Some(attr) = f.attrs.iter().find(|a| a.path().is_ident("read")) {
        if let Meta::List(meta) = attr.meta.clone() {
            meta.parse_nested_meta(|meta| {
                if meta.path.is_ident("as") {
                    let value = meta.value()?;
                    strategy = Some(value.parse()?);
                    Ok(())
                } else if meta.path.is_ident("bound") {
                    let value = meta.value()?;
                    let int_lit: syn::LitInt = value.parse()?;
                    bound = Some(int_lit);
                    Ok(())
                } else {
                    Err(meta.error(UNSUPPORTED_READ_PROP))
                }
            })
            .unwrap_or_else(|e| panic!("Failed to parse `read` attribute: {e}"));
        } else {
            panic!("{WRONG_READ_FORMAT}");
        }
    }

    FieldReadAttributes { strategy, bound }
}

/// Generates read code for a field based on the given strategy.
fn generate_read_code(
    strategy: &Strategy,
    field_type: &syn::Type,
    bound: Option<&syn::LitInt>,
) -> proc_macro2::TokenStream {
    match strategy.name_str().as_str() {
        "VarInt" => quote! {
            steel_utils::codec::VarInt::read(data)?.0 as #field_type
        },
        "VarLong" => quote! {
            steel_utils::codec::VarLong::read(data)?.0 as #field_type
        },
        "Prefixed" => {
            let prefix = strategy
                .prefix_type_tokens()
                .unwrap_or_else(|| quote! { steel_utils::codec::VarInt });

            if let Some(inner) = &strategy.inner {
                // Custom inner read strategy - read length then iterate
                let inner_read = generate_read_code(inner, field_type, None);
                quote! {
                    {
                        use steel_utils::serial::PrefixedRead;
                        let len = #prefix::read(data)?.0 as usize;
                        let mut items = Vec::with_capacity(len);
                        for _ in 0..len {
                            items.push(#inner_read);
                        }
                        items
                    }
                }
            } else {
                // Default: use PrefixedRead trait
                if let Some(b) = bound {
                    quote! {
                        {
                            use steel_utils::serial::PrefixedRead;
                            <#field_type>::read_prefixed_bound::<#prefix>(data, #b)?
                        }
                    }
                } else {
                    quote! {
                        {
                            use steel_utils::serial::PrefixedRead;
                            <#field_type>::read_prefixed::<#prefix>(data)?
                        }
                    }
                }
            }
        }
        "Unprefixed" => {
            // For Option<T>: read inner value directly (caller handles presence)
            if let Some(inner) = &strategy.inner {
                let inner_read = generate_read_code(inner, field_type, None);
                quote! {
                    Some(#inner_read)
                }
            } else {
                quote! {
                    Some(<#field_type>::read(data)?)
                }
            }
        }
        s => panic!(
            "Unknown read strategy: `{s}`. \
            Expected one of: VarInt, VarLong, Prefixed, Unprefixed"
        ),
    }
}

/// Parses struct-level read attributes for newtypes.
fn parse_struct_read_attributes(attrs: &[syn::Attribute]) -> FieldReadAttributes {
    let mut strategy: Option<Strategy> = None;
    let mut bound: Option<syn::LitInt> = None;

    if let Some(attr) = attrs.iter().find(|a| a.path().is_ident("read")) {
        if let Meta::List(meta) = attr.meta.clone() {
            meta.parse_nested_meta(|meta| {
                if meta.path.is_ident("as") {
                    let value = meta.value()?;
                    strategy = Some(value.parse()?);
                    Ok(())
                } else if meta.path.is_ident("bound") {
                    let value = meta.value()?;
                    let int_lit: syn::LitInt = value.parse()?;
                    bound = Some(int_lit);
                    Ok(())
                } else {
                    Err(meta.error(UNSUPPORTED_READ_PROP))
                }
            })
            .unwrap_or_else(|e| panic!("Failed to parse `read` attribute: {e}"));
        } else {
            panic!("{WRONG_READ_FORMAT}");
        }
    }

    FieldReadAttributes { strategy, bound }
}

fn read_from_struct(s: syn::DataStruct, name: Ident, attrs: &[syn::Attribute]) -> TokenStream {
    match s.fields {
        Fields::Named(fields) => {
            // Create read calls for every field
            let readers = fields.named.iter().map(|f| {
                let field_name = f.ident.as_ref().expect("should have a named field");
                let field_type = &f.ty;
                let FieldReadAttributes { strategy, bound } = parse_read_attributes(f);

                if let Some(strat) = strategy {
                    let read_code = generate_read_code(&strat, field_type, bound.as_ref());
                    quote! {
                        let #field_name = #read_code;
                    }
                } else {
                    quote! {
                        let #field_name = <#field_type>::read(data)?;
                    }
                }
            });

            let field_names = fields
                .named
                .iter()
                .map(|f| f.ident.as_ref().expect("should have a named field"));

            let expanded = quote! {
                #[automatically_derived]
                impl steel_utils::serial::ReadFrom for #name {
                    fn read(data: &mut std::io::Cursor<&[u8]>) -> std::io::Result<Self>{
                        use steel_utils::serial::PrefixedRead;

                        #(#readers)*

                        Ok(Self {
                            #(#field_names),*
                        })
                    }
                }
            };

            TokenStream::from(expanded)
        }
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
            // Newtype: single unnamed field
            let field_type = &fields.unnamed.first().expect("checked len == 1").ty;
            let FieldReadAttributes { strategy, bound } = parse_struct_read_attributes(attrs);

            let read_code = if let Some(strat) = strategy {
                generate_read_code(&strat, field_type, bound.as_ref())
            } else {
                quote! { <#field_type>::read(data)? }
            };

            let expanded = quote! {
                #[automatically_derived]
                impl steel_utils::serial::ReadFrom for #name {
                    fn read(data: &mut std::io::Cursor<&[u8]>) -> std::io::Result<Self> {
                        use steel_utils::serial::PrefixedRead;

                        Ok(Self(#read_code))
                    }
                }
            };

            TokenStream::from(expanded)
        }
        Fields::Unnamed(_) => {
            panic!("Read only supports tuple structs with a single field (newtypes)");
        }
        Fields::Unit => {
            // Unit struct: read nothing
            let expanded = quote! {
                #[automatically_derived]
                impl steel_utils::serial::ReadFrom for #name {
                    fn read(_data: &mut std::io::Cursor<&[u8]>) -> std::io::Result<Self> {
                        Ok(Self)
                    }
                }
            };

            TokenStream::from(expanded)
        }
    }
}

fn read_from_enum(e: syn::DataEnum, name: Ident, attrs: Vec<syn::Attribute>) -> TokenStream {
    let readers = e.variants.iter().map(|v| {
        assert!(
            matches!(v.fields, Fields::Unit),
            "Read only supports enum variants without fields"
        );
        let Some((_, value)) = &v.discriminant else {
            panic!(
                "Read only supports enum variants with explicit discriminant\n(Ej. {} = 0)",
                v.ident
            )
        };
        let v_name = &v.ident;
        quote! {
            #value => #name::#v_name,
        }
    });

    // Support reading the enum discriminant using a specified strategy
    // Defaults to reading a varint when no attribute is provided
    let mut strategy: Option<Strategy> = None;
    let mut bound: Option<syn::LitInt> = None;

    if let Some(attr) = attrs.iter().find(|a| a.path().is_ident("read")) {
        if let Meta::List(meta) = attr.meta.clone() {
            meta.parse_nested_meta(|meta| {
                if meta.path.is_ident("as") {
                    let value = meta.value()?;
                    strategy = Some(value.parse()?);
                    Ok(())
                } else if meta.path.is_ident("bound") {
                    let value = meta.value()?;
                    let int_lit: syn::LitInt = value.parse()?;
                    bound = Some(int_lit);
                    Ok(())
                } else {
                    Err(meta.error(UNSUPPORTED_READ_PROP))
                }
            })
            .unwrap_or_else(|e| panic!("Failed to parse `read` attribute: {e}"));
        } else {
            panic!("{WRONG_READ_FORMAT}");
        }
    }

    let read_discriminant = match &strategy.as_ref().map(Strategy::name_str) {
        // Default: read a VarInt (i32)
        None => {
            quote! { steel_utils::codec::VarInt::read(data)?.into() }
        }
        // Explicit VarInt
        Some(s) if s == "VarInt" => {
            quote! { steel_utils::codec::VarInt::read(data)?.into() }
        }
        // VarLong
        Some(s) if s == "VarLong" => {
            quote! { steel_utils::codec::VarLong::read(data)?.into() }
        }
        // Primitive numeric type (u8, i32, etc.)
        Some(s) if ALLOWED_TYPES.contains(&s.as_str()) => {
            let enum_type = Ident::new(s, Span::call_site());
            let _ = bound; // `bound` currently unused for primitive reads
            quote! { <#enum_type as steel_utils::serial::ReadFrom>::read(data)? }
        }
        Some(s) => panic!(
            "Unknown read strategy for enum: `{s}`. \
            Expected one of: VarInt, VarLong, or a primitive type ({ALLOWED_TYPES:?})"
        ),
    };

    let error_msg = format!("Invalid {name}");

    TokenStream::from(quote! {
        #[automatically_derived]
        impl steel_utils::serial::ReadFrom for #name {
            fn read(data: &mut std::io::Cursor<&[u8]>) -> std::io::Result<Self> {
                Ok(match { #read_discriminant } {
                    #(#readers)*
                    _ => {
                        return Err(
                            std::io::Error::other(#error_msg)
                        );
                    }
                })
            }
        }
    })
}
