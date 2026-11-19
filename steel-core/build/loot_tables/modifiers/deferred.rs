use super::super::{
    Compiler, TokenStream,
    model::{CompileModifier, Condition, ContextIntProvider, Holder, HolderSet, StewEffect},
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

// TODO: Select a stew effect with the loot RNG, evaluate its duration with
// vanilla instant-effect units, and append it to suspicious-stew effects.
deferred!(SetStewEffect, "minecraft:set_stew_effect" { effects: Option<Vec<StewEffect>> });
