//! Build-time generation for vanilla registry tags.

mod block;
mod common;
mod fluid;
mod item;

use heck::{ToShoutySnakeCase, ToUpperCamelCase};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use steel_utils::Identifier;

pub(crate) use block::build as block;
pub(crate) use fluid::build as fluid;
pub(crate) use item::build as item;

/// Returns the name of the generated constant for a tag, as the tag generators name it.
fn const_ident(tag: &Identifier) -> Ident {
    let name = if tag.namespace == Identifier::VANILLA_NAMESPACE {
        tag.path.to_shouty_snake_case()
    } else {
        format!("{}:{}", tag.namespace, tag.path).to_shouty_snake_case()
    };
    Ident::new(&name, Span::call_site())
}

/// References the generated constant for `tag` in a registry's tag module, such as `"item"`.
///
/// Generated data that names a tag missing from the extracted tags fails to compile.
pub(crate) fn const_path(registry_module: &str, tag: &Identifier) -> TokenStream {
    let module = Ident::new(
        &format!("vanilla_{registry_module}_tags"),
        Span::call_site(),
    );
    let tag_type = Ident::new(
        &format!("{}Tag", registry_module.to_upper_camel_case()),
        Span::call_site(),
    );
    let tag = const_ident(tag);
    quote! { crate::#module::#tag_type::#tag }
}

macro_rules! simple_tag_builder {
    ($function:ident, $tag_subdir:literal, $registry_module:literal, $registry_type:literal) => {
        pub(crate) fn $function() -> TokenStream {
            common::build_simple_tags($tag_subdir, $registry_module, $registry_type)
        }
    };
}

simple_tag_builder!(
    banner_pattern,
    "banner_pattern",
    "banner_pattern",
    "BannerPatternRegistry"
);
simple_tag_builder!(biome, "worldgen/biome", "biome", "BiomeRegistry");
simple_tag_builder!(
    damage_type,
    "damage_type",
    "damage_type",
    "DamageTypeRegistry"
);
simple_tag_builder!(dialog, "dialog", "dialog", "DialogRegistry");
simple_tag_builder!(
    enchantment,
    "enchantment",
    "enchantment",
    "EnchantmentRegistry"
);
simple_tag_builder!(
    entity_type,
    "entity_type",
    "entity_type",
    "EntityTypeRegistry"
);
simple_tag_builder!(instrument, "instrument", "instrument", "InstrumentRegistry");
simple_tag_builder!(
    painting_variant,
    "painting_variant",
    "painting_variant",
    "PaintingVariantRegistry"
);
simple_tag_builder!(poi_type, "point_of_interest_type", "poi", "PoiTypeRegistry");
simple_tag_builder!(potion, "potion", "potion", "PotionRegistry");
simple_tag_builder!(
    structure,
    "worldgen/structure",
    "structure",
    "StructureRegistry"
);
simple_tag_builder!(timeline, "timeline", "timeline", "TimelineRegistry");
