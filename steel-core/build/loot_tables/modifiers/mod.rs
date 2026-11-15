use super::{
    Compiler, Ident, TokenStream,
    model::{Condition, ContextIntProvider, Holder},
    quote,
};
use enum_dispatch::enum_dispatch;
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use std::fmt;

mod bonus;
mod components;
mod control;
mod count;
mod deferred;

pub use bonus::ApplyBonus;
pub use components::{CopyComponents, SetEnchantments, SetInstrument};
pub use control::{Discard, ExplosionDecay, Filtered, FurnaceSmelt, Sequence};
pub use count::{EnchantedCountIncrease, LimitCount, SetCount, SetOminousBottleAmplifier};
pub use deferred::{
    CopyState, EnchantRandomly, EnchantWithLevels, ExplorationMap, SetComponents, SetDamage,
    SetName, SetPotion, SetStewEffect,
};

impl Compiler {
    pub(super) fn modifier(&mut self, modifier: Option<&Modifier>) -> Ident {
        let Some(modifier) = modifier else {
            return Ident::new("identity", proc_macro2::Span::call_site());
        };
        let name = self.name("modifier");
        let body = self.modifier_body(modifier);
        self.definitions.extend(quote! {
            fn #name(ctx: &mut LootContext<'_>, item: &mut ItemStack) -> LootResult<()> {
                let _ = (&ctx, &item);
                #body
                Ok(())
            }
        });
        name
    }

    fn modifier_body(&mut self, modifier: &Modifier) -> TokenStream {
        match modifier {
            Modifier::Sequence(modifiers) => {
                let functions = modifiers
                    .iter()
                    .map(|modifier| self.modifier(Some(modifier)))
                    .collect::<Vec<_>>();
                quote! { #(#functions(ctx, item)?;)* }
            }
            Modifier::Reference(key) => {
                self.with_reference("item_modifier", key, Self::modifier_body)
            }
            Modifier::Inline(function) => self.modifier_function(function),
        }
    }

    fn modifier_function(&mut self, function: &ModifierFunction) -> TokenStream {
        let body = function.compile(self);
        if let Some(condition) = function.condition() {
            let condition = self.condition(Some(condition));
            quote! { let passes = #condition; if passes { #body } }
        } else {
            body
        }
    }

    fn unsupported_modifier(operation: &str) -> TokenStream {
        quote! { ctx.unsupported::<()>(#operation)?; }
    }
}

#[enum_dispatch]
pub(super) trait CompileModifier {
    fn compile(&self, compiler: &mut Compiler) -> TokenStream;
}

#[enum_dispatch]
pub(super) trait Conditioned {
    fn condition(&self) -> Option<&Holder<Condition>>;
}

macro_rules! conditioned {
    ($($modifier:ty),* $(,)?) => {$(
        impl Conditioned for $modifier {
            fn condition(&self) -> Option<&Holder<Condition>> { self.condition.as_ref() }
        }
    )*};
}

conditioned!(
    ApplyBonus,
    CopyComponents,
    CopyState,
    Discard,
    EnchantRandomly,
    EnchantWithLevels,
    EnchantedCountIncrease,
    ExplorationMap,
    ExplosionDecay,
    Filtered,
    FurnaceSmelt,
    LimitCount,
    Sequence,
    SetComponents,
    SetCount,
    SetDamage,
    SetEnchantments,
    SetInstrument,
    SetName,
    SetOminousBottleAmplifier,
    SetPotion,
    SetStewEffect,
);

pub enum Modifier {
    Reference(String),
    Inline(Box<ModifierFunction>),
    Sequence(Vec<Modifier>),
}

impl<'de> Deserialize<'de> for Modifier {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ModifierVisitor;
        impl<'de> Visitor<'de> for ModifierVisitor {
            type Value = Modifier;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an item modifier, reference, or modifier list")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(Modifier::Reference(value.to_owned()))
            }
            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
                ModifierFunction::deserialize(de::value::MapAccessDeserializer::new(map))
                    .map(|function| Modifier::Inline(Box::new(function)))
            }
            fn visit_seq<S: SeqAccess<'de>>(self, sequence: S) -> Result<Self::Value, S::Error> {
                Vec::deserialize(de::value::SeqAccessDeserializer::new(sequence))
                    .map(Modifier::Sequence)
            }
        }
        deserializer.deserialize_any(ModifierVisitor)
    }
}

// TODO: Add vanilla modifiers: copy_custom_data, copy_name, fill_player_head,
// modify_contents, set_attributes, set_banner_pattern, set_book_cover, set_contents,
// set_custom_data, set_custom_model_data, set_firework_explosion, set_fireworks,
// set_item, set_loot_table, set_lore, set_random_dyes, set_random_potion,
// set_writable_book_pages, set_written_book_pages, and toggle_tooltips.
#[enum_dispatch(CompileModifier, Conditioned)]
#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ModifierFunction {
    #[serde(rename = "minecraft:set_count")]
    SetCount(SetCount),
    #[serde(rename = "minecraft:explosion_decay")]
    ExplosionDecay(ExplosionDecay),
    #[serde(rename = "minecraft:discard")]
    Discard(Discard),
    #[serde(rename = "minecraft:apply_bonus")]
    ApplyBonus(ApplyBonus),
    #[serde(rename = "minecraft:enchanted_count_increase")]
    EnchantedCountIncrease(EnchantedCountIncrease),
    #[serde(rename = "minecraft:limit_count")]
    LimitCount(LimitCount),
    #[serde(rename = "minecraft:furnace_smelt")]
    FurnaceSmelt(FurnaceSmelt),
    #[serde(rename = "minecraft:set_ominous_bottle_amplifier")]
    SetOminousBottleAmplifier(SetOminousBottleAmplifier),
    #[serde(rename = "minecraft:set_instrument")]
    SetInstrument(SetInstrument),
    #[serde(rename = "minecraft:set_enchantments")]
    SetEnchantments(SetEnchantments),
    #[serde(rename = "minecraft:filtered")]
    Filtered(Filtered),
    #[serde(rename = "minecraft:sequence")]
    Sequence(Sequence),
    #[serde(rename = "minecraft:copy_components")]
    CopyComponents(CopyComponents),
    #[serde(rename = "minecraft:enchant_randomly")]
    EnchantRandomly(EnchantRandomly),
    #[serde(rename = "minecraft:enchant_with_levels")]
    EnchantWithLevels(EnchantWithLevels),
    #[serde(rename = "minecraft:copy_state")]
    CopyState(CopyState),
    // These payloads belong to deferred gameplay codecs. Do not imply support
    // by decoding them as a generic successful operation.
    #[serde(rename = "minecraft:set_components")]
    SetComponents(SetComponents),
    #[serde(rename = "minecraft:set_damage")]
    SetDamage(SetDamage),
    #[serde(rename = "minecraft:exploration_map")]
    ExplorationMap(ExplorationMap),
    #[serde(rename = "minecraft:set_name")]
    SetName(SetName),
    #[serde(rename = "minecraft:set_potion")]
    SetPotion(SetPotion),
    #[serde(rename = "minecraft:set_stew_effect")]
    SetStewEffect(SetStewEffect),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StewEffect {
    #[serde(rename = "type")]
    _effect: String,
    #[serde(rename = "duration")]
    _duration: ContextIntProvider,
}
