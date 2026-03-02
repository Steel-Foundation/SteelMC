//! # Steel Macros
//!
//! Macros for the Steel Minecraft server.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    Data, DeriveInput, Fields, Ident, Meta,
    parse::{Parse, ParseStream},
    parse_macro_input,
    token::Paren,
};

const ALLOWED_TYPES: [&str; 12] = [
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
];

const UNSUPPORTED_READ_PROP: &str =
    "unsupported property. Supported properties are `as = ...`, `bound = ...`";
const UNSUPPORTED_WRITE_PROP: &str =
    "unsupported property. Supported properties are `as = ...`, `bound = ...`";
const WRONG_READ_FORMAT: &str = "attribute requires a list format: `#[read(as = ..., bound = ..)]";
const WRONG_WRITE_FORMAT: &str =
    "attribute requires a list format: `#[write(as = ..., bound = ..)]";

/// Represents a parsed strategy from read/write attributes.
///
/// Supports:
/// - Simple: `VarInt`, `Byte`, `Json`
/// - Container: `Prefixed(VarInt)`, `Prefixed(VarInt, inner = VarInt)`
/// - Unprefixed: `Unprefixed`, `Unprefixed(inner = VarInt)`
#[derive(Clone)]
struct Strategy {
    name: Ident,
    /// For Prefixed: the prefix type (e.g., `VarInt`, u16)
    prefix_type: Option<syn::Type>,
    /// For container strategies: how to read/write inner elements
    inner: Option<Box<Strategy>>,
}

impl Strategy {
    fn name_str(&self) -> String {
        self.name.to_string()
    }

    /// Gets the prefix type as a token stream, expanding known identifiers to full paths.
    fn prefix_type_tokens(&self) -> Option<proc_macro2::TokenStream> {
        self.prefix_type.as_ref().map(expand_known_type)
    }
}

/// Expands known type identifiers to their fully qualified paths.
///
/// For example, `VarInt` becomes `steel_utils::codec::VarInt`.
fn expand_known_type(ty: &syn::Type) -> proc_macro2::TokenStream {
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

/// Derives the `ReadFrom` trait for a struct.
///
/// # Panics
/// - If the derive macro is used on a union.
/// - If the derive macro is used on a struct with unnamed fields.
/// - If the `read` attribute is malformed.
/// - If an unknown read strategy is specified.
#[proc_macro_derive(ReadFrom, attributes(read))]
pub fn read_from_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    match input.data {
        Data::Struct(s) => read_from_struct(s, name, &input.attrs),
        Data::Enum(e) => read_from_enum(e, name, input.attrs),
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
                &v.ident
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

/// Derives the `WriteTo` trait for a struct.
///
/// # Panics
/// - If the derive macro is used on a union.
/// - If the derive macro is used on a struct with unnamed fields.
/// - If the `write` attribute is malformed.
/// - If an unknown write strategy is specified.
#[proc_macro_derive(WriteTo, attributes(write))]
pub fn write_to_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    match input.data {
        Data::Struct(s) => write_to_struct(s, name, &input.generics, &input.attrs),
        Data::Enum(_) => write_to_enum(name, input.attrs),
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
#[allow(clippy::too_many_lines)]
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

#[allow(clippy::too_many_lines)]
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

fn write_to_enum(name: Ident, attrs: Vec<syn::Attribute>) -> TokenStream {
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
            Expected one of: VarInt, Prefixed, or a primitive type ({ALLOWED_TYPES:?})"
        ),
    };

    TokenStream::from(quote! {
        #[automatically_derived]
        impl steel_utils::serial::WriteTo for #name {
            fn write(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
                #writer

                Ok(())
            }
        }
    })
}

/// Derives the `ClientPacket` trait for a struct.
///
/// # Panics
/// - If the `packet_id` attribute is missing or malformed.
#[proc_macro_derive(ClientPacket, attributes(packet_id))]
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
#[proc_macro_derive(ServerPacket, attributes(packet_id))]
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
