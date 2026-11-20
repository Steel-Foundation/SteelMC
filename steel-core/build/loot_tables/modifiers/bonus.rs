use super::super::{
    Compiler, TokenStream, identifier,
    model::{CompileModifier, Condition, Holder},
    quote,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyBonus {
    pub enchantment: String,
    pub formula: BonusFormula,
    pub parameters: Option<BonusParameters>,
    pub condition: Option<Holder<Condition>>,
}

#[derive(Deserialize)]
pub enum BonusFormula {
    #[serde(rename = "minecraft:ore_drops")]
    OreDrops,
    #[serde(rename = "minecraft:uniform_bonus_count")]
    Uniform,
    #[serde(rename = "minecraft:binomial_with_bonus_count")]
    Binomial,
}

#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum BonusParameters {
    Uniform {
        #[serde(rename = "bonusMultiplier")]
        multiplier: i32,
    },
    Binomial {
        extra: i32,
        probability: f32,
    },
}

impl CompileModifier for ApplyBonus {
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        let enchantment = identifier(&self.enchantment);
        let calculate = match (&self.formula, &self.parameters) {
            (BonusFormula::OreDrops, None) => quote! {
                if level > 0 { let bonus = (ctx.rng.next_i32_bounded(level + 2) - 1).max(0);
                    item.set_count(item.count().wrapping_mul(bonus + 1)); }
            },
            (BonusFormula::Uniform, Some(BonusParameters::Uniform { multiplier })) => {
                quote! { crate::loot::modifiers::uniform_bonus(item, ctx.rng, level, #multiplier); }
            }
            (BonusFormula::Binomial, Some(BonusParameters::Binomial { extra, probability })) => {
                quote! {
                    for _ in 0..level + #extra { if ctx.rng.next_f32() < #probability { item.grow(1); } }
                }
            }
            _ => panic!("bonus formula requires its matching parameters"),
        };
        quote! { if ctx.tool.is_some() { let level = ctx.get_enchantment_level_by_id(&#enchantment); #calculate } }
    }
}
