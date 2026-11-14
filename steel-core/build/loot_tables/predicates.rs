use super::{
    Compiler, TokenStream, ident, identifier,
    model::{Bounds, IntRange, yes},
    quote,
};
use heck::ToUpperCamelCase;
use quote::format_ident;
use serde::{Deserialize, Deserializer, de::IgnoredAny};
use std::collections::BTreeMap;

impl Compiler {
    pub(super) fn item_predicate(predicate: &ItemPredicate) -> TokenStream {
        let mut checks = Vec::new();
        if let Some(items) = &predicate.items {
            if let Some(tag) = items.strip_prefix('#') {
                let tag = identifier(tag);
                checks.push(quote! { REGISTRY.items.is_in_tag(item.item, &#tag) });
            } else {
                let item = ident(items);
                checks.push(quote! { item.is(&vanilla_items::#item) });
            }
        }
        if let Some(count) = &predicate.count {
            match count {
                IntRange::Exact(count) => checks.push(quote! { item.count() == #count }),
                IntRange::Bounds(bounds) => {
                    if let Some(min) = bounds.min {
                        checks.push(quote! {item.count() >= #min});
                    }
                    if let Some(max) = bounds.max {
                        checks.push(quote! {item.count() <= #max});
                    }
                }
            }
        }
        if let Some(predicates) = &predicate.predicates {
            let mut components = BTreeMap::new();
            if let Some(enchantments) = &predicates.enchantments {
                // TODO: Require the enchantments component even for an empty list.
                let checks = enchantments.iter().map(|enchantment| {
                    let key = &enchantment.enchantments;
                    let tag = key.starts_with('#');
                    let key = identifier(key.trim_start_matches('#'));
                    let min = enchantment.levels.as_ref().and_then(|levels| levels.min).unwrap_or(0);
                    let max = enchantment.levels.as_ref().and_then(|levels| levels.max).unwrap_or(i32::MAX);
                    quote! { crate::loot::predicates::item_enchantment_matches(item, &#key, #tag, #min, #max) }
                }).collect::<Vec<_>>();
                components.insert("minecraft:enchantments", checks);
            }
            for key in predicates.presence.keys() {
                // TODO: Distinguish registered typed component predicates from
                // component-presence predicates and validate their component keys.
                let id = identifier(key);
                components.insert(key.as_str(), vec![quote! { item.has_component(&#id) }]);
            }
            checks.extend(components.into_values().flatten());
        }
        Self::all(&checks)
    }

    pub(super) fn entity_predicate(predicate: &EntityPredicate) -> TokenStream {
        let mut checks = Vec::new();
        if let Some(kind) = &predicate.entity_type {
            if let Some(tag) = kind.strip_prefix('#') {
                let tag = identifier(tag);
                checks.push(quote! { entity.entity_type.and_then(|key| REGISTRY.entity_types.by_key(key)).is_some_and(|kind| REGISTRY.entity_types.is_in_tag(kind, &#tag)) });
            } else {
                let kind = identifier(kind);
                checks.push(quote! { entity.entity_type == Some(&#kind) });
            }
        }
        // Keep short-circuit order stable across the typed input representation.
        if let Some(components) = &predicate.components {
            let mut component_checks = BTreeMap::new();
            if let Some(color) = &components.sheep_color {
                component_checks.insert("minecraft:sheep/color", quote! { entity.sheep_color.is_some_and(|color| color.serialized_name() == #color) });
            }
            if let Some(variant) = &components.chicken_variant {
                let variant = identifier(variant);
                component_checks.insert(
                    "minecraft:chicken/variant",
                    quote! { entity.chicken_variant == Some(&#variant) },
                );
            }
            if let Some(variant) = &components.mooshroom_variant {
                component_checks.insert(
                    "minecraft:mooshroom/variant",
                    quote! { entity.mooshroom_variant.is_some_and(|variant| variant.serialized_name() == #variant) },
                );
            }
            for name in components.unsupported.keys() {
                // TODO: Match other typed entity components through their entity
                // capabilities, including frog variants used by extracted tables.
                let operation = format!("entity component {name}");
                component_checks.insert(
                    name.as_str(),
                    quote! { ctx.unsupported::<bool>(#operation)? },
                );
            }
            checks.extend(component_checks.into_values());
        }
        if let Some(equipment) = &predicate.equipment {
            for (slot, predicate) in equipment {
                let slot = format_ident!("{}", slot.replace("hand", "_hand").to_upper_camel_case());
                let predicate = Self::item_predicate(predicate);
                checks.push(quote! { crate::loot::predicates::equipment_matches(entity, steel_registry::equipment::EquipmentSlot::#slot, |item| #predicate) });
            }
        }
        if let Some(flags) = &predicate.flags {
            for (flag, value) in flags {
                let flag = format_ident!("{flag}");
                checks.push(Self::expected(*value, quote! { entity.flags.#flag }));
            }
        }
        for (key, present) in [
            (
                "minecraft:type_specific/cube_mob",
                predicate.cube_mob.is_some(),
            ),
            (
                "minecraft:type_specific/fishing_hook",
                predicate.fishing_hook.is_some(),
            ),
            ("minecraft:type_specific/raider", predicate.raider.is_some()),
        ] {
            if present {
                // TODO: Decode and match cube-mob size, fishing-hook open-water,
                // and raider raid/captain predicates through entity capabilities.
                let operation = format!("entity predicate {key}");
                checks.push(quote! { ctx.unsupported::<bool>(#operation)? });
            }
        }
        if let Some(sheep) = &predicate.sheep {
            checks.push(sheep.sheared.map_or_else(
                || quote! { entity.sheep_sheared.is_some() },
                |sheared| quote! { entity.sheep_sheared == Some(#sheared) },
            ));
        }
        if predicate.vehicle.is_some() {
            // TODO: Resolve the entity's actual vehicle and evaluate its predicate.
            checks.push(quote! { ctx.unsupported::<bool>("entity predicate minecraft:vehicle")? });
        }
        Self::all(&checks)
    }

    pub(super) fn damage_source_predicate(predicate: &DamageSourcePredicate) -> TokenStream {
        // TODO: Match vanilla's evaluation order: tags, direct entity, source
        // entity, then is_direct.
        let mut checks = Vec::new();
        for tag in &predicate.tags {
            let key = identifier(tag.id.strip_prefix('#').unwrap_or(&tag.id));
            checks.push(Self::expected(
                tag.expected,
                quote! { crate::loot::predicates::damage_has_tag(source, &#key) },
            ));
        }
        if let Some(value) = predicate.is_direct {
            checks.push(Self::expected(value, quote! { source.is_direct }));
        }
        for (predicate, entity) in [
            (predicate.source_entity.as_ref(), quote! {ctx.killer_entity}),
            (
                predicate.direct_entity.as_ref(),
                quote! {ctx.direct_killer_entity},
            ),
        ] {
            if let Some(predicate) = predicate {
                let check = Self::entity_predicate(predicate);
                checks.push(quote! { match #entity { Some(entity) => { let _ = &entity; #check }, None => false } });
            }
        }
        let checks = Self::all(&checks);
        quote! { match ctx.damage_source { Some(source) => { let _ = &source; #checks }, None => false } }
    }

    pub(super) fn all(terms: &[TokenStream]) -> TokenStream {
        Self::join(terms, &quote! { && }, &quote! { true })
    }
    pub(super) fn any(terms: &[TokenStream]) -> TokenStream {
        Self::join(terms, &quote! { || }, &quote! { false })
    }
    fn join(terms: &[TokenStream], op: &TokenStream, empty: &TokenStream) -> TokenStream {
        match terms {
            [] => empty.clone(),
            [term] => term.clone(),
            [first, rest @ ..] => quote! { (#first) #(#op (#rest))* },
        }
    }

    fn expected(expected: bool, check: TokenStream) -> TokenStream {
        if expected {
            check
        } else {
            quote! { !#check }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemPredicate {
    // TODO: Support vanilla holder-set lists and exact component predicates.
    pub items: Option<String>,
    pub count: Option<IntRange>,
    pub predicates: Option<ItemComponentPredicates>,
}

#[derive(Deserialize)]
pub struct ItemComponentPredicates {
    #[serde(rename = "minecraft:enchantments")]
    pub enchantments: Option<Vec<EnchantmentPredicate>>,
    // The currently supported non-enchantment predicates require an empty
    // payload. Unknown nonempty codecs fail decoding instead of becoming presence checks.
    #[expect(
        clippy::zero_sized_map_values,
        reason = "JSON objects map component keys to validated empty predicate payloads"
    )]
    #[serde(flatten)]
    pub presence: BTreeMap<String, EmptyPredicate>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyPredicate {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnchantmentPredicate {
    // TODO: Support optional enchantment holder sets and exact/ranged levels.
    pub enchantments: String,
    pub levels: Option<Bounds<i32>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityPredicate {
    // TODO: Add vanilla entity subpredicates: distance, effects, entity_tags,
    // location, movement, movement_affected_by, nbt, passenger, periodic_tick,
    // predicates, slots, stepping_on, targeted_entity, team,
    // type_specific/lightning, and type_specific/player.
    #[serde(rename = "minecraft:entity_type")]
    pub entity_type: Option<String>,
    #[serde(rename = "minecraft:flags")]
    pub flags: Option<BTreeMap<String, bool>>,
    #[serde(rename = "minecraft:equipment")]
    pub equipment: Option<BTreeMap<String, ItemPredicate>>,
    #[serde(rename = "minecraft:components")]
    pub components: Option<EntityComponents>,
    #[serde(rename = "minecraft:type_specific/sheep")]
    pub sheep: Option<SheepPredicate>,
    #[serde(
        default,
        rename = "minecraft:vehicle",
        deserialize_with = "deferred_predicate"
    )]
    pub vehicle: Option<IgnoredAny>,
    #[serde(
        default,
        rename = "minecraft:type_specific/cube_mob",
        deserialize_with = "deferred_predicate"
    )]
    pub cube_mob: Option<IgnoredAny>,
    #[serde(
        default,
        rename = "minecraft:type_specific/fishing_hook",
        deserialize_with = "deferred_predicate"
    )]
    pub fishing_hook: Option<IgnoredAny>,
    #[serde(
        default,
        rename = "minecraft:type_specific/raider",
        deserialize_with = "deferred_predicate"
    )]
    pub raider: Option<IgnoredAny>,
}

// Even a null payload must retain the unsupported key instead of becoming an
// absent predicate through Option's usual null handling.
fn deferred_predicate<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<IgnoredAny>, D::Error> {
    IgnoredAny::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
pub struct EntityComponents {
    #[serde(rename = "minecraft:sheep/color")]
    pub sheep_color: Option<String>,
    #[serde(rename = "minecraft:chicken/variant")]
    pub chicken_variant: Option<String>,
    #[serde(rename = "minecraft:mooshroom/variant")]
    pub mooshroom_variant: Option<String>,
    // Component payloads without a supported entity capability remain explicit
    // runtime errors, including dynamically named component types.
    #[expect(
        clippy::zero_sized_map_values,
        reason = "deferred component payloads are discarded but their object keys must be retained for errors"
    )]
    #[serde(flatten)]
    pub unsupported: BTreeMap<String, IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheepPredicate {
    pub sheared: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DamageSourcePredicate {
    #[serde(default)]
    pub tags: Vec<TagPredicate>,
    pub source_entity: Option<EntityPredicate>,
    pub direct_entity: Option<EntityPredicate>,
    pub is_direct: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagPredicate {
    pub id: String,
    #[serde(default = "yes")]
    pub expected: bool,
}
