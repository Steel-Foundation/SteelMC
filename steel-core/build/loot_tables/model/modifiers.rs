pub use super::super::modifiers::{
    ApplyBonus, BinomialWithBonusCount, CopyComponents, CopyState, Discard, EnchantRandomly,
    EnchantWithLevels, EnchantedCountIncrease, ExplorationMap, ExplosionDecay, Filtered,
    FurnaceSmelt, LimitCount, OreDrops, Sequence, SetComponents, SetCount, SetDamage,
    SetEnchantments, SetInstrument, SetName, SetOminousBottleAmplifier, SetPotion, SetStewEffect,
    UniformBonusCount,
};
use super::super::{Compiler, TokenStream};
use super::{Condition, ContextIntProvider, Holder};
use enum_dispatch::enum_dispatch;
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use std::fmt;

#[enum_dispatch]
pub(in super::super) trait CompileModifier {
    fn condition(&self) -> Option<&Holder<Condition>>;
    fn compile(&self, compiler: &mut Compiler) -> TokenStream;
}

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
#[enum_dispatch(CompileModifier)]
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

#[enum_dispatch]
pub(in super::super) trait CompileBonusFormula {
    fn compile(&self) -> TokenStream;
}

#[enum_dispatch(CompileBonusFormula)]
pub enum BonusFormula {
    OreDrops(OreDrops),
    Uniform(UniformBonusCount),
    Binomial(BinomialWithBonusCount),
}
