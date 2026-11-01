use super::super::{
    Compiler, TokenStream, identifier,
    model::{BonusFormula, CompileBonusFormula, CompileModifier, Condition, Holder},
    quote,
};
use serde::{Deserialize, Deserializer, de};

pub struct ApplyBonus {
    pub enchantment: String,
    pub formula: BonusFormula,
    pub condition: Option<Holder<Condition>>,
}

impl<'de> Deserialize<'de> for ApplyBonus {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        enum FormulaKind {
            #[serde(rename = "minecraft:ore_drops")]
            OreDrops,
            #[serde(rename = "minecraft:uniform_bonus_count")]
            Uniform,
            #[serde(rename = "minecraft:binomial_with_bonus_count")]
            Binomial,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Parameters {
            Uniform(UniformParameters),
            Binomial(BinomialParameters),
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct UniformParameters {
            #[serde(rename = "bonusMultiplier")]
            multiplier: i32,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct BinomialParameters {
            extra: i32,
            probability: f32,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            enchantment: String,
            condition: Option<Holder<Condition>>,
            formula: FormulaKind,
            parameters: Option<Parameters>,
        }
        let fields = Fields::deserialize(deserializer)?;
        let formula = match (fields.formula, fields.parameters) {
            (FormulaKind::OreDrops, None) => OreDrops {}.into(),
            (FormulaKind::Uniform, Some(Parameters::Uniform(p))) => UniformBonusCount {
                multiplier: p.multiplier,
            }
            .into(),
            (FormulaKind::Binomial, Some(Parameters::Binomial(p))) => BinomialWithBonusCount {
                extra: p.extra,
                probability: p.probability,
            }
            .into(),
            _ => {
                return Err(de::Error::custom(
                    "bonus formula requires its matching parameters",
                ));
            }
        };
        Ok(Self {
            enchantment: fields.enchantment,
            condition: fields.condition,
            formula,
        })
    }
}

impl CompileModifier for ApplyBonus {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        let enchantment = identifier(&self.enchantment);
        let calculate = self.formula.compile();
        quote! { if ctx.tool.is_some() { let level = ctx.get_enchantment_level_by_id(&#enchantment); #calculate } }
    }
}

pub struct OreDrops {}

impl CompileBonusFormula for OreDrops {
    fn compile(&self) -> TokenStream {
        quote! {
            if level > 0 { let bonus = (ctx.rng.next_i32_bounded(level + 2) - 1).max(0);
                item.set_count(item.count().wrapping_mul(bonus + 1)); }
        }
    }
}
pub struct UniformBonusCount {
    pub multiplier: i32,
}

impl CompileBonusFormula for UniformBonusCount {
    fn compile(&self) -> TokenStream {
        let multiplier = self.multiplier;
        quote! { crate::loot::modifiers::uniform_bonus(item, ctx.rng, level, #multiplier); }
    }
}
pub struct BinomialWithBonusCount {
    pub extra: i32,
    pub probability: f32,
}

impl CompileBonusFormula for BinomialWithBonusCount {
    fn compile(&self) -> TokenStream {
        let Self { extra, probability } = self;
        quote! { for _ in 0..level + #extra { if ctx.rng.next_f32() < #probability { item.grow(1); } } }
    }
}
