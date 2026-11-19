use std::collections::BTreeMap;

use serde::{Deserialize, de::IgnoredAny};

use super::super::{
    Compiler, TokenStream, ident, identifier,
    model::{CompileCondition, HolderSet},
    quote,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocationCheck {
    #[serde(default, rename = "offsetX")]
    offset_x: i32,
    #[serde(default, rename = "offsetY")]
    offset_y: i32,
    #[serde(default, rename = "offsetZ")]
    offset_z: i32,
    predicate: Option<LocationPredicate>,
}

/// Implements the biome predicates required by abandoned-camp maps. Other
/// location fields retain explicit errors until their gameplay support exists.
#[derive(Deserialize)]
struct LocationPredicate {
    biomes: Option<HolderSet>,
    #[expect(
        clippy::zero_sized_map_values,
        reason = "unsupported payloads are discarded while field names are retained for errors"
    )]
    #[serde(flatten)]
    unsupported: BTreeMap<String, IgnoredAny>,
}

impl CompileCondition for LocationCheck {
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        let Some(predicate) = &self.predicate else {
            return quote! { ctx.origin.is_some() };
        };
        if let Some(field) = predicate.unsupported.keys().next() {
            let operation = format!("minecraft:location_check predicate {field}");
            return quote! { ctx.origin.is_some() && ctx.unsupported::<bool>(#operation)? };
        }
        let Some(biomes) = &predicate.biomes else {
            return quote! { ctx.origin.is_some() };
        };
        let matches = match biomes {
            HolderSet::Single(key) if key.starts_with('#') => {
                let tag = identifier(&key[1..]);
                quote! { REGISTRY.biomes.is_in_tag(biome, &#tag) }
            }
            HolderSet::Single(key) => Self::biome_match(key),
            HolderSet::List(keys) => {
                let checks = keys
                    .iter()
                    .map(|key| Self::biome_match(key))
                    .collect::<Vec<_>>();
                Compiler::any(&checks)
            }
        };
        let binding = if matches!(biomes, HolderSet::List(keys) if keys.is_empty()) {
            quote! { _ }
        } else {
            quote! { biome }
        };
        let Self {
            offset_x,
            offset_y,
            offset_z,
            ..
        } = self;
        quote! {
            if let Some(origin) = ctx.origin {
                let Some(world) = ctx.world else {
                    return ctx.unsupported("minecraft:location_check requires a world");
                };
                let pos = steel_utils::BlockPos::containing(
                    origin.x + f64::from(#offset_x),
                    origin.y + f64::from(#offset_y),
                    origin.z + f64::from(#offset_z),
                );
                crate::loot::predicates::location_biome(world, pos)
                    .is_some_and(|#binding| #matches)
            } else { false }
        }
    }
}

impl LocationCheck {
    fn biome_match(key: &str) -> TokenStream {
        assert!(
            !key.starts_with('#'),
            "biome holder lists cannot contain tags"
        );
        let biome = ident(key);
        quote! { biome == &*steel_registry::vanilla_biomes::#biome }
    }
}
