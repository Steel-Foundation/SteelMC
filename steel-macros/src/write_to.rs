use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Field, Fields, Ident, Meta, Variant, parse_macro_input};

use crate::strategy::{ALLOWED_TYPES, Strategy};

const UNSUPPORTED_WRITE_PROP: &str =
    "unsupported property. Supported properties are `as = ...`, `bound = ...`";
const WRONG_WRITE_FORMAT: &str =
    "attribute requires a list format: `#[write(as = ..., bound = ..)]";

pub(super) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    match input.data {
        Data::Struct(value) => write_to_struct(value, name, &input.generics, &input.attrs),
        Data::Enum(value) => write_to_enum(value, name, &input.generics, input.attrs),
        Data::Union(_) => panic!("Write can only be derived for structs and enums"),
    }
}

struct FieldWriteAttributes {
    strategy: Option<Strategy>,
    bound: Option<syn::LitInt>,
}

fn parse_write_attributes(f: &syn::Field) -> FieldWriteAttributes {
    let mut strategy: Option<Strategy> = None;
    let mut bound: Option<syn::LitInt> = None;

    if let Some(attr) = f.attrs.iter().find(|a| a.path().is_ident("write")) {
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
                    Err(meta.error(UNSUPPORTED_WRITE_PROP))
                }
            })
            .unwrap_or_else(|e| panic!("Failed to parse `write` attribute: {e}"));
        } else {
            panic!("{WRONG_WRITE_FORMAT}");
        }
    }

    FieldWriteAttributes { strategy, bound }
}

/// Generates write code for a value based on the given strategy.
///
/// # Arguments
/// - `strategy`: The write strategy to apply
/// - `value`: Token stream representing the value to write (e.g., `self.field` or `item`)
/// - `bound`: Optional bound for prefixed writes
#[expect(
    clippy::too_many_lines,
    reason = "2 lines over; splitting would hurt readability"
)]
fn generate_write_code(
    strategy: &Strategy,
    value: proc_macro2::TokenStream,
    bound: Option<&syn::LitInt>,
) -> proc_macro2::TokenStream {
    match strategy.name_str().as_str() {
        "VarInt" => quote! {
            steel_utils::codec::VarInt(#value as i32).write(writer)?;
        },
        "VarLong" => quote! {
            steel_utils::codec::VarLong(#value as i64).write(writer)?;
        },
        "Byte" => quote! {
            (#value as i8).write(writer)?;
        },
        "I64" => quote! {
            (#value).as_i64().write(writer)?;
        },
        "Json" => {
            let prefix = strategy
                .prefix_type_tokens()
                .unwrap_or_else(|| quote! { steel_utils::codec::VarInt });
            quote! {
                {
                    use steel_utils::serial::PrefixedWrite;
                    serde_json::to_string(&#value).map_err(|e| {
                        std::io::Error::other(format!("Failed to serialize: {e}"))
                    })?.write_prefixed::<#prefix>(writer)?;
                }
            }
        }
        "OptionByte" => quote! {
            if let Some(value) = &#value {
                (*value as i8).write(writer)?;
            } else {
                (-1i8).write(writer)?;
            }
        },
        // Registry holder reference format: write (id + 1) as VarInt
        // Minecraft uses 0 for "direct" (inline value) and N>0 for "reference" (registry id = N-1)
        "RegistryHolder" => quote! {
            steel_utils::codec::VarInt((#value) as i32 + 1).write(writer)?;
        },
        "Prefixed" => {
            let prefix = strategy
                .prefix_type_tokens()
                .unwrap_or_else(|| quote! { steel_utils::codec::VarInt });

            if let Some(inner) = &strategy.inner {
                // Custom inner write strategy - iterate and apply
                let inner_write = generate_write_code(inner, quote! { *item }, None);
                quote! {
                    {
                        use steel_utils::serial::PrefixedWrite;
                        #prefix::from((#value).len() as i32).write(writer)?;
                        for item in &#value {
                            #inner_write
                        }
                    }
                }
            } else {
                // Default: use PrefixedWrite trait
                let write_call = if let Some(b) = bound {
                    quote! { (#value).write_prefixed_bound::<#prefix>(writer, #b)?; }
                } else {
                    quote! { (#value).write_prefixed::<#prefix>(writer)?; }
                };
                quote! {
                    {
                        use steel_utils::serial::PrefixedWrite;
                        #write_call
                    }
                }
            }
        }
        "Unprefixed" => {
            // For Option<T>: write inner value if Some, nothing if None
            if let Some(inner) = &strategy.inner {
                let inner_write = generate_write_code(inner, quote! { *inner_value }, None);
                quote! {
                    if let Some(inner_value) = &#value {
                        #inner_write
                    }
                }
            } else {
                // Default: just call write on inner if Some
                quote! {
                    if let Some(inner_value) = &#value {
                        inner_value.write(writer)?;
                    }
                }
            }
        }
        "NoPrefixVec" => {
            // Write vec items without length prefix
            if let Some(inner) = &strategy.inner {
                let inner_write = generate_write_code(inner, quote! { *item }, None);
                quote! {
                    for item in &#value {
                        #inner_write
                    }
                }
            } else {
                quote! {
                    for item in &#value {
                        item.write(writer)?;
                    }
                }
            }
        }
        s => panic!(
            "Unknown write strategy: `{s}`. \
            Expected one of: VarInt, VarLong, Byte, I64, Json, OptionByte, RegistryHolder, Prefixed, Unprefixed, NoPrefixVec"
        ),
    }
}

/// Parses struct-level write attributes for newtypes.
fn parse_struct_write_attributes(attrs: &[syn::Attribute]) -> FieldWriteAttributes {
    let mut strategy: Option<Strategy> = None;
    let mut bound: Option<syn::LitInt> = None;

    if let Some(attr) = attrs.iter().find(|a| a.path().is_ident("write")) {
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
                    Err(meta.error(UNSUPPORTED_WRITE_PROP))
                }
            })
            .unwrap_or_else(|e| panic!("Failed to parse `write` attribute: {e}"));
        } else {
            panic!("{WRONG_WRITE_FORMAT}");
        }
    }

    FieldWriteAttributes { strategy, bound }
}

fn write_to_struct(
    s: syn::DataStruct,
    name: Ident,
    generics: &syn::Generics,
    attrs: &[syn::Attribute],
) -> TokenStream {
    let (impl_generics, ty_generics, _) = generics.split_for_impl();

    match s.fields {
        Fields::Named(fields) => {
            let writers = fields.named.iter().map(|f| {
                let field_name = f.ident.as_ref().expect("should have a named field");
                let FieldWriteAttributes { strategy, bound } = parse_write_attributes(f);

                if let Some(strat) = strategy {
                    generate_write_code(&strat, quote! { self.#field_name }, bound.as_ref())
                } else {
                    quote! {
                        self.#field_name.write(writer)?;
                    }
                }
            });

            let expanded = quote! {
                #[automatically_derived]
                impl #impl_generics steel_utils::serial::WriteTo for #name #ty_generics {
                    fn write(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
                        #(#writers)*

                        Ok(())
                    }
                }
            };

            TokenStream::from(expanded)
        }
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
            // Newtype: single unnamed field
            let FieldWriteAttributes { strategy, bound } = parse_struct_write_attributes(attrs);

            let writer = if let Some(strat) = strategy {
                generate_write_code(&strat, quote! { self.0 }, bound.as_ref())
            } else {
                quote! {
                    self.0.write(writer)?;
                }
            };

            let expanded = quote! {
                #[automatically_derived]
                impl #impl_generics steel_utils::serial::WriteTo for #name #ty_generics {
                    fn write(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
                        #writer

                        Ok(())
                    }
                }
            };

            TokenStream::from(expanded)
        }
        Fields::Unnamed(_) => {
            panic!("Write only supports tuple structs with a single field (newtypes)");
        }
        Fields::Unit => {
            // Unit struct: write nothing
            let expanded = quote! {
                #[automatically_derived]
                impl #impl_generics steel_utils::serial::WriteTo for #name #ty_generics {
                    fn write(&self, _writer: &mut impl std::io::Write) -> std::io::Result<()> {
                        Ok(())
                    }
                }
            };

            TokenStream::from(expanded)
        }
    }
}

fn write_to_enum(
    s: syn::DataEnum,
    name: Ident,
    generics: &syn::Generics,
    attrs: Vec<syn::Attribute>,
) -> TokenStream {
    let (impl_generics, ty_generics, _) = generics.split_for_impl();

    let mut strategy: Option<Strategy> = None;
    let mut bound: Option<syn::LitInt> = None;

    if let Some(attr) = attrs.iter().find(|a| a.path().is_ident("write")) {
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
                    Err(meta.error(UNSUPPORTED_WRITE_PROP))
                }
            })
            .unwrap_or_else(|e| panic!("Failed to parse `write` attribute: {e}"));
        } else {
            panic!("{WRONG_WRITE_FORMAT}");
        }
    } else {
        panic!("WriteTo for enums requires the `write` attribute: #[write(as = VarInt)]")
    }

    let strategy = strategy.expect("WriteTo for enums requires `as = ...` in the write attribute");
    let strategy_name = strategy.name_str();

    let writer = match strategy_name.as_str() {
        // Write enum discriminant as VarInt
        "VarInt" => {
            quote! {
                steel_utils::codec::VarInt(*self as i32).write(writer)?;
            }
        }
        // Write enum as prefixed string (for string-based enums)
        "Prefixed" => {
            let prefix = strategy
                .prefix_type_tokens()
                .unwrap_or_else(|| quote! { steel_utils::codec::VarInt });

            let write_call = if let Some(b) = bound {
                quote! { self.write_prefixed_bound::<#prefix>(writer, #b)?; }
            } else {
                quote! { self.write_prefixed::<#prefix>(writer)?; }
            };

            quote! {
                {
                    use steel_utils::serial::PrefixedWrite;
                    #write_call
                }
            }
        }
        // Write enum with dispatch based on enum variant
        "Dispatched" => {
            let branches = s
                .variants
                .into_iter()
                .enumerate()
                .map(|(ordinal, variant)| dispatch_enum_variant_match_branch(ordinal, variant));

            quote! {
                match self {
                    #(#branches),*
                }
            }
        }
        // Write as primitive numeric type (u8, i32, etc.)
        s if ALLOWED_TYPES.contains(&s) => {
            let enum_type = Ident::new(s, Span::call_site());
            let _ = bound; // `bound` currently unused for primitive writes
            quote! {
                (*self as #enum_type).write(writer)?;
            }
        }
        s => panic!(
            "Unknown write strategy for enum: `{s}`. \
            Expected one of: VarInt, Prefixed, Dispatched, or a primitive type ({ALLOWED_TYPES:?})"
        ),
    };

    TokenStream::from(quote! {
        #[automatically_derived]
        impl #impl_generics steel_utils::serial::WriteTo for #name #ty_generics {
            fn write(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
                #writer

                Ok(())
            }
        }
    })
}

fn dispatch_enum_variant_match_branch(
    ordinal: usize,
    variant: Variant,
) -> proc_macro2::TokenStream {
    let variant_ident = variant.ident;
    let ordinal =
        i32::try_from(ordinal).expect("enum variant ordinal is too large to fit in an i32");

    let ordinal_writer = quote! {
        steel_utils::codec::VarInt(#ordinal).write(writer)?;
    };

    match variant.fields {
        Fields::Named(fields) => {
            let writers = dispatch_enum_variant_field_writers(&fields.named);
            let field_names = fields.named.iter().enumerate().map(|(i, f)| {
                let field_name = f.ident.as_ref().expect("should have a named field");
                // Prevent name collisions
                let local_var_name = format_ident!("value{i}");
                quote! {
                    #field_name: #local_var_name
                }
            });

            quote! {
                Self::#variant_ident { #(#field_names),* } => {
                    #ordinal_writer
                    #(#writers)*
                }
            }
        }
        Fields::Unnamed(fields) => {
            let writers = dispatch_enum_variant_field_writers(&fields.unnamed);
            let values = (0..fields.unnamed.len()).map(|i| format_ident!("value{i}"));

            quote! {
                Self::#variant_ident( #(#values),* ) => {
                    #ordinal_writer
                    #(#writers)*
                }
            }
        }
        Fields::Unit => {
            quote! {
                Self::#variant_ident => { #ordinal_writer }
            }
        }
    }
}

fn dispatch_enum_variant_field_writers<'a>(
    iter: impl IntoIterator<Item = &'a Field>,
) -> impl Iterator<Item = proc_macro2::TokenStream> {
    iter.into_iter().enumerate().map(|(i, f)| {
        let FieldWriteAttributes { strategy, bound } = parse_write_attributes(f);
        let ident = format_ident!("value{i}");

        if let Some(strat) = strategy {
            generate_write_code(&strat, quote! { (*#ident) }, bound.as_ref())
        } else {
            quote! {
                #ident.write(writer)?;
            }
        }
    })
}
