pub use super::super::numbers::{
    FloatConstant, FloatUniform, IntBinomial, IntConstant, IntUniform, LinearLevelBasedValue,
};
use super::super::{Compiler, TokenStream};
use enum_dispatch::enum_dispatch;
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, Visitor},
};
use std::{fmt, marker::PhantomData};

#[enum_dispatch]
pub(in super::super) trait CompileLevelBasedFunction {
    fn compile(&self) -> TokenStream;
}

#[enum_dispatch]
pub(in super::super) trait CompileFloatProvider {
    fn compile(&self, compiler: &mut Compiler) -> TokenStream;
}

#[enum_dispatch]
pub(in super::super) trait CompileIntProvider {
    fn compile(&self, compiler: &mut Compiler) -> TokenStream;
}

/// The numeric literal, reference, and inline forms share shape dispatch while
/// integer and float function enums keep their distinct vanilla semantics.
pub enum NumberProvider<N, F> {
    Constant(N),
    Reference(String),
    Inline(Box<F>),
}

impl<'de, N: Deserialize<'de>, F: Deserialize<'de>> Deserialize<'de> for NumberProvider<N, F> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ProviderVisitor<N, F>(PhantomData<(N, F)>);
        impl<'de, N: Deserialize<'de>, F: Deserialize<'de>> Visitor<'de> for ProviderVisitor<N, F> {
            type Value = NumberProvider<N, F>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a numeric literal, provider reference, or typed provider")
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                N::deserialize(de::value::I64Deserializer::new(value)).map(NumberProvider::Constant)
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                N::deserialize(de::value::U64Deserializer::new(value)).map(NumberProvider::Constant)
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
                N::deserialize(de::value::F64Deserializer::new(value)).map(NumberProvider::Constant)
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(NumberProvider::Reference(value.to_owned()))
            }
            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
                F::deserialize(de::value::MapAccessDeserializer::new(map))
                    .map(|function| NumberProvider::Inline(Box::new(function)))
            }
        }
        deserializer.deserialize_any(ProviderVisitor(PhantomData))
    }
}

pub type ContextIntProvider = NumberProvider<i32, IntProvider>;
pub type ContextFloatProvider = NumberProvider<f32, FloatProvider>;

// TODO: Add vanilla integer providers: abs, add, avg, conditional, div,
// environment_attribute, floor_div, floor_mod, from_float, max, min, mod, mul,
// negate, number_dispatcher, pow, score, storage, sub, and weighted_list.
#[enum_dispatch(CompileIntProvider)]
#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum IntProvider {
    #[serde(rename = "minecraft:constant")]
    Constant(IntConstant),
    #[serde(rename = "minecraft:uniform")]
    Uniform(IntUniform),
    #[serde(rename = "minecraft:binomial")]
    Binomial(IntBinomial),
}

// TODO: Add vanilla float providers: abs, add, avg, ceil, conditional, cos, div,
// enchantment_level, environment_attribute, floor, from_int, length, max, min, mod,
// mul, negate, number_dispatcher, pow, round, sin, sqrt, storage, sub, truncate,
// and weighted_list.
#[enum_dispatch(CompileFloatProvider)]
#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum FloatProvider {
    #[serde(rename = "minecraft:constant")]
    Constant(FloatConstant),
    #[serde(rename = "minecraft:uniform")]
    Uniform(FloatUniform),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum LevelBasedValue {
    Constant(f32),
    Function(LevelBasedFunction),
}

// TODO: Add vanilla level-based values: clamped, exponent, fraction,
// levels_squared, and lookup.
#[enum_dispatch(CompileLevelBasedFunction)]
#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum LevelBasedFunction {
    #[serde(rename = "minecraft:linear")]
    Linear(LinearLevelBasedValue),
}
