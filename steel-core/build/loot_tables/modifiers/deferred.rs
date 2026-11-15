use super::super::{
    Compiler, TokenStream,
    model::{
        CompileModifier, Condition, ContextFloatProvider, ContextIntProvider, Holder, HolderSet,
        StewEffect,
    },
};
use serde::Deserialize;
use serde::de::IgnoredAny;

/// Decodes a modifier whose gameplay is not implemented and compiles it to an
/// explicit unsupported-operation error.
macro_rules! deferred {
    ($name:ident, $operation:literal { $($(#[$attr:meta])* $field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        #[expect(dead_code, reason = "payload is validated but not yet applied")]
        pub struct $name {
            $($(#[$attr])* $field: $ty,)*
            pub condition: Option<Holder<Condition>>,
        }

        impl CompileModifier for $name {
            fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
                Compiler::unsupported_modifier($operation)
            }
        }
    };
}

// TODO: Decode compatibility/additional-cost options and implement seeded
// enchantment selection, level rolls, and book conversion.
deferred!(EnchantRandomly, "minecraft:enchant_randomly" { options: Option<HolderSet> });

// TODO: Evaluate the level provider and implement vanilla enchanting with
// configured options, book conversion, and additional-cost handling.
deferred!(EnchantWithLevels, "minecraft:enchant_with_levels" {
    levels: Option<ContextIntProvider>,
    options: Option<HolderSet>,
});

// TODO: Copy the selected context block-state properties into the item's
// block_state component using the configured block's property definitions.
deferred!(CopyState, "minecraft:copy_state" {
    block: Option<String>,
    properties: Option<Vec<String>>,
});

// TODO: Decode typed component patches and apply them with item validation.
deferred!(SetComponents, "minecraft:set_components" { components: Option<IgnoredAny> });

// TODO: Apply remaining-durability fractions with vanilla additive mode,
// clamping, rounding, and damageable-item checks.
deferred!(SetDamage, "minecraft:set_damage" {
    damage: Option<ContextFloatProvider>,
    #[serde(default)]
    add: bool,
});

// TODO: Decode destination holder sets and search_radius; locate and claim
// structures from the loot origin, create saved map data, render its biome
// preview, and add the configured target decoration and zoom.
deferred!(ExplorationMap, "minecraft:exploration_map" {
    destination: Option<String>,
    decoration: Option<String>,
    zoom: Option<i32>,
    skip_existing_chunks: Option<bool>,
});

// TODO: Decode and resolve text against the selected context entity, then
// apply it to the configured item-name or custom-name component.
deferred!(SetName, "minecraft:set_name" {
    name: Option<IgnoredAny>,
    target: Option<String>,
});

// TODO: Resolve the typed potion and update potion_contents while
// preserving its other fields.
deferred!(SetPotion, "minecraft:set_potion" { id: Option<String> });

// TODO: Select a stew effect with the loot RNG, evaluate its duration with
// vanilla instant-effect units, and append it to suspicious-stew effects.
deferred!(SetStewEffect, "minecraft:set_stew_effect" { effects: Option<Vec<StewEffect>> });
