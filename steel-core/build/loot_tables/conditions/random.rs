use super::super::{
    Compiler, TokenStream, identifier,
    model::{CompileCondition, CompileLevelBasedFunction, ContextFloatProvider, LevelBasedValue},
    quote,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RandomChance {
    pub chance: ContextFloatProvider,
}

impl CompileCondition for RandomChance {
    fn compile(&self, compiler: &mut Compiler) -> TokenStream {
        let Self { chance, .. } = self;
        let chance = compiler.float_provider(chance);
        quote! {{ let probability = #chance; ctx.rng.next_f32() < probability }}
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableBonus {
    pub enchantment: String,
    pub chances: Vec<f32>,
}

impl CompileCondition for TableBonus {
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        let Self {
            enchantment,
            chances,
            ..
        } = self;
        let enchantment = identifier(enchantment);
        assert!(!chances.is_empty(), "Empty table_bonus chances");
        quote! {{ let chances = [#(#chances),*]; let level = ctx.get_enchantment_level_by_id(&#enchantment).max(0) as usize;
        ctx.rng.next_f32() < chances[level.min(chances.len() - 1)] }}
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RandomChanceWithEnchantedBonus {
    pub enchantment: String,
    pub unenchanted_chance: f32,
    pub enchanted_chance: LevelBasedValue,
}

impl CompileCondition for RandomChanceWithEnchantedBonus {
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        let Self {
            enchantment,
            unenchanted_chance,
            enchanted_chance,
            ..
        } = self;
        let enchantment = identifier(enchantment);
        let chance = match enchanted_chance {
            LevelBasedValue::Constant(value) => quote! { #value },
            LevelBasedValue::Function(function) => function.compile(),
        };
        quote! {{ let level = ctx.attacker_enchantment_level(&#enchantment);
        let chance = if level > 0 { #chance } else { #unenchanted_chance }; ctx.rng.next_f32() < chance }}
    }
}
