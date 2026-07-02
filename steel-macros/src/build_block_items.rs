use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Fields, ImplItem, ItemImpl, ItemStruct,
    meta::{ParseNestedMeta, parser},
    parse::Parser,
    parse2,
};

const KNOWN_ENTITY_CLASSES: &[&str] = &[
    "entity",
    "player",
    "living",
    "mob",
    "pathfinder_mob",
    "ageable_mob",
    "animal",
];

const KNOWN_ENTITY_INTERFACES: &[&str] = &["item_steerable"];

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
/// Adds `Entity::capabilities` from the entity's vanilla class and implemented
/// vanilla interfaces. The generated method uses ordinary trait-object
/// coercions, so missing trait impls fail at compile time instead of turning
/// into silent runtime misses.
pub fn entity_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    let metadata = parse_entity_metadata(attr);
    let mut input: ItemImpl = parse2(item)
        .unwrap_or_else(|_| panic!("#[entity_impl] can only be applied to impl blocks"));

    assert_entity_impl(&input);
    assert!(
        !input.items.iter().any(|item| {
            matches!(item, ImplItem::Fn(function) if function.sig.ident == "capabilities")
        }),
        "#[entity_impl] generates Entity::capabilities; remove the manual implementation"
    );

    let capabilities = metadata.capabilities();
    let requirements = metadata.requirements();
    let requirement_checks = requirements
        .iter()
        .map(|requirement| match requirement.as_str() {
            "ageable_mob" => quote! {
                let _: &dyn crate::entity::AgeableMob = self;
            },
            _ => panic!("unknown generated entity requirement `{requirement}`"),
        });
    let assignments = capabilities.iter().map(|capability| {
        let setter = format_ident!("with_{capability}");
        quote! {
            capabilities = capabilities.#setter(self);
        }
    });

    let method: ImplItem = parse2(quote! {
        fn capabilities(&self) -> crate::entity::EntityCapabilities<'_> {
            #(#requirement_checks)*
            let mut capabilities = crate::entity::EntityCapabilities::none();
            #(#assignments)*
            capabilities
        }
    })
    .unwrap_or_else(|error| panic!("generated Entity::capabilities should parse: {error}"));

    input.items.push(method);

    if !input
        .items
        .iter()
        .any(|item| matches!(item, ImplItem::Fn(function) if function.sig.ident == "base_tick"))
        && let Some(base_tick_method) = metadata.base_tick_method()
    {
        let method: ImplItem = parse2(base_tick_method)
            .unwrap_or_else(|error| panic!("generated Entity::base_tick should parse: {error}"));
        input.items.push(method);
    }

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

#[derive(Clone, Copy)]
enum EntityClass {
    Entity,
    Player,
    Living,
    Mob,
    PathfinderMob,
    AgeableMob,
    Animal,
}

impl EntityClass {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "entity" => Self::Entity,
            "player" => Self::Player,
            "living" => Self::Living,
            "mob" => Self::Mob,
            "pathfinder_mob" => Self::PathfinderMob,
            "ageable_mob" => Self::AgeableMob,
            "animal" => Self::Animal,
            _ => return None,
        })
    }

    const fn capabilities(self) -> &'static [&'static str] {
        match self {
            Self::Entity => &[],
            Self::Player => &["player", "living"],
            Self::Living => &["living"],
            Self::Mob => &["living", "mob"],
            Self::PathfinderMob | Self::AgeableMob => &["living", "mob", "pathfinder_mob"],
            Self::Animal => &["living", "mob", "pathfinder_mob", "animal"],
        }
    }

    const fn requirements(self) -> &'static [&'static str] {
        match self {
            Self::AgeableMob => &["ageable_mob"],
            _ => &[],
        }
    }

    fn base_tick_method(self) -> Option<TokenStream> {
        match self {
            Self::Entity => None,
            Self::Player | Self::Living => Some(quote! {
                fn base_tick(&self) {
                    crate::entity::LivingEntity::base_tick_living_entity(self);
                }
            }),
            Self::Mob | Self::PathfinderMob | Self::AgeableMob | Self::Animal => Some(quote! {
                fn base_tick(&self) {
                    crate::entity::Mob::base_tick_mob(self);
                }
            }),
        }
    }
}

struct EntityMetadata {
    class: EntityClass,
    interfaces: Vec<String>,
}

impl EntityMetadata {
    fn capabilities(&self) -> Vec<String> {
        let mut capabilities = Vec::new();
        for capability in self.class.capabilities() {
            push_unique(&mut capabilities, capability);
        }
        for interface in &self.interfaces {
            push_unique(&mut capabilities, interface);
        }
        capabilities
    }

    fn requirements(&self) -> Vec<String> {
        let mut requirements = Vec::new();
        for requirement in self.class.requirements() {
            push_unique(&mut requirements, requirement);
        }
        requirements
    }

    fn base_tick_method(&self) -> Option<TokenStream> {
        self.class.base_tick_method()
    }
}

fn push_unique(capabilities: &mut Vec<String>, capability: &str) {
    if !capabilities.iter().any(|existing| existing == capability) {
        capabilities.push(capability.to_owned());
    }
}

fn parse_single_nested_ident(meta: ParseNestedMeta<'_>, context: &str) -> syn::Result<String> {
    let mut parsed = None;
    meta.parse_nested_meta(|nested| {
        if parsed.is_some() {
            return Err(nested.error(format!("{context} accepts exactly one identifier")));
        }
        let Some(ident) = nested.path.get_ident() else {
            return Err(nested.error(format!("{context} must be an identifier")));
        };
        parsed = Some(ident.to_string());
        Ok(())
    })?;

    parsed.ok_or_else(|| meta.error(format!("{context} requires an identifier")))
}

fn parse_entity_metadata(attr: TokenStream) -> EntityMetadata {
    let mut class = None;
    let mut interfaces = Vec::new();
    let attr_parser = parser(|meta| {
        if meta.path.is_ident("class") {
            if class.is_some() {
                return Err(meta.error("duplicate entity class"));
            }
            let class_name = parse_single_nested_ident(meta, "class(...)")?;
            let parsed_class = EntityClass::parse(&class_name).ok_or_else(|| {
                syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "unknown entity class `{class_name}`; expected one of: {}",
                        KNOWN_ENTITY_CLASSES.join(", ")
                    ),
                )
            })?;
            class = Some(parsed_class);
            return Ok(());
        }

        if meta.path.is_ident("interfaces") {
            meta.parse_nested_meta(|nested| {
                let Some(ident) = nested.path.get_ident() else {
                    return Err(nested.error("entity interface must be an identifier"));
                };
                let interface = ident.to_string();
                if !KNOWN_ENTITY_INTERFACES.contains(&interface.as_str()) {
                    return Err(nested.error(format!(
                        "unknown entity interface `{interface}`; expected one of: {}",
                        KNOWN_ENTITY_INTERFACES.join(", ")
                    )));
                }
                if interfaces.contains(&interface) {
                    return Err(nested.error(format!("duplicate entity interface `{interface}`")));
                }
                interfaces.push(interface);
                Ok(())
            })?;
            return Ok(());
        }

        Err(meta.error("expected `class(...)` or `interfaces(...)`"))
    });

    attr_parser
        .parse2(attr)
        .unwrap_or_else(|error| panic!("Failed to parse entity_impl attribute: {error}"));

    let Some(class) = class else {
        panic!(
            "#[entity_impl] requires `class(...)` with one of: {}",
            KNOWN_ENTITY_CLASSES.join(", ")
        );
    };

    EntityMetadata { class, interfaces }
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
