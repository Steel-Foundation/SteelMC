use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Fields, ImplItem, ItemImpl, ItemStruct, meta::parser, parse::Parser, parse2};

const KNOWN_ENTITY_CAPABILITIES: &[&str] = &[
    "player",
    "living",
    "mob",
    "pathfinder_mob",
    "animal",
    "item_steerable",
];

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

/// Attribute macro for entity behavior structs.
///
/// Strips `#[json_arg(...)]` field attributes (which are only read by the build script
/// scanning source files) and passes the struct through unchanged.
pub fn entity_behavior(_attr: TokenStream, item: TokenStream) -> TokenStream {
    strip_json_arg_attrs(item, "entity_behavior")
}

/// Attribute macro for `impl Entity` blocks.
///
/// Adds `Entity::capabilities` from an explicit capability list. The generated
/// method uses ordinary trait-object coercions, so missing trait impls fail at
/// compile time instead of turning into silent runtime misses.
pub fn entity_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    let capabilities = parse_entity_capabilities(attr);
    let mut input: ItemImpl = parse2(item)
        .unwrap_or_else(|_| panic!("#[entity_impl] can only be applied to impl blocks"));

    assert_entity_impl(&input);
    assert!(
        !input.items.iter().any(|item| {
            matches!(item, ImplItem::Fn(function) if function.sig.ident == "capabilities")
        }),
        "#[entity_impl] generates Entity::capabilities; remove the manual implementation"
    );

    let assignments = capabilities.iter().map(|capability| {
        let setter = format_ident!("with_{capability}");
        quote! {
            capabilities = capabilities.#setter(self);
        }
    });

    let method: ImplItem = parse2(quote! {
        fn capabilities(&self) -> crate::entity::EntityCapabilities<'_> {
            let mut capabilities = crate::entity::EntityCapabilities::none();
            #(#assignments)*
            capabilities
        }
    })
    .expect("generated Entity::capabilities should parse");

    input.items.push(method);
    quote! { #input }
}

fn assert_entity_impl(input: &ItemImpl) {
    let Some((_, trait_path, _)) = &input.trait_ else {
        panic!("#[entity_impl] can only be applied to `impl Entity for ...` blocks");
    };

    assert!(
        trait_path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Entity"),
        "#[entity_impl] can only be applied to `impl Entity for ...` blocks"
    );
}

fn parse_entity_capabilities(attr: TokenStream) -> Vec<String> {
    let mut capabilities = Vec::new();
    let attr_parser = parser(|meta| {
        if !meta.path.is_ident("capabilities") {
            return Err(meta.error("expected `capabilities(...)`"));
        }

        meta.parse_nested_meta(|nested| {
            let Some(ident) = nested.path.get_ident() else {
                return Err(nested.error("entity capability must be an identifier"));
            };
            let capability = ident.to_string();
            if !KNOWN_ENTITY_CAPABILITIES.contains(&capability.as_str()) {
                return Err(nested.error(format!(
                    "unknown entity capability `{capability}`; expected one of: {}",
                    KNOWN_ENTITY_CAPABILITIES.join(", ")
                )));
            }
            if capabilities.contains(&capability) {
                return Err(nested.error(format!("duplicate entity capability `{capability}`")));
            }
            capabilities.push(capability);
            Ok(())
        })
    });

    attr_parser
        .parse2(attr)
        .unwrap_or_else(|error| panic!("Failed to parse entity_impl attribute: {error}"));
    assert!(
        !capabilities.is_empty(),
        "#[entity_impl] requires `capabilities(...)` with at least one capability"
    );
    capabilities
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
