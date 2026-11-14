use super::{Compiler, TokenStream, quote};
use enum_dispatch::enum_dispatch;
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, Visitor},
};
use std::{fmt, marker::PhantomData};

impl Compiler {
    pub(super) fn int_provider(&mut self, provider: &ContextIntProvider) -> TokenStream {
        match provider {
            NumberProvider::Constant(value) => quote! { #value },
            NumberProvider::Reference(key) => {
                self.with_reference("context_int_provider", key, Self::int_provider)
            }
            NumberProvider::Inline(function) => function.compile(self),
        }
    }
    pub(super) fn float_provider(&mut self, provider: &ContextFloatProvider) -> TokenStream {
        // TODO: Preserve vanilla's distinct unsafe, finite-or-zero, and throwing
        // float-provider evaluation semantics at their respective call sites.
        match provider {
            NumberProvider::Constant(value) => quote! { #value },
            NumberProvider::Reference(key) => {
                self.with_reference("context_float_provider", key, Self::float_provider)
            }
            NumberProvider::Inline(function) => function.compile(self),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntConstant {
    pub value: ContextIntProvider,
}

impl CompileNumberProvider for IntConstant {
    fn compile(&self, compiler: &mut Compiler) -> TokenStream {
        compiler.int_provider(&self.value)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntUniform {
    pub min: ContextIntProvider,
    pub max: ContextIntProvider,
}

impl CompileNumberProvider for IntUniform {
    fn compile(&self, compiler: &mut Compiler) -> TokenStream {
        let min = compiler.int_provider(&self.min);
        let max = compiler.int_provider(&self.max);
        quote! {{ let min = #min; let max = #max;
            if min >= max { min } else { ctx.rng.next_i32_between(min, max) }
        }}
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntBinomial {
    pub n: ContextIntProvider,
    pub p: ContextFloatProvider,
}

impl CompileNumberProvider for IntBinomial {
    fn compile(&self, compiler: &mut Compiler) -> TokenStream {
        let n = compiler.int_provider(&self.n);
        let p = compiler.float_provider(&self.p);
        quote! {{ let n = #n; let p = #p; let mut count = 0;
            for _ in 0..n { if ctx.rng.next_f32() < p { count += 1; } } count
        }}
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FloatConstant {
    pub value: ContextFloatProvider,
}

impl CompileNumberProvider for FloatConstant {
    fn compile(&self, compiler: &mut Compiler) -> TokenStream {
        compiler.float_provider(&self.value)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FloatUniform {
    pub min: ContextFloatProvider,
    pub max: ContextFloatProvider,
}

impl CompileNumberProvider for FloatUniform {
    fn compile(&self, compiler: &mut Compiler) -> TokenStream {
        let min = compiler.float_provider(&self.min);
        let max = compiler.float_provider(&self.max);
        quote! {{ let min = #min; let max = #max;
            if min >= max { min } else { ctx.rng.next_f32() * (max - min) + min }
        }}
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinearLevelBasedValue {
    pub base: f32,
    pub per_level_above_first: f32,
}

impl CompileLevelBasedFunction for LinearLevelBasedValue {
    fn compile(&self) -> TokenStream {
        let Self {
            base,
            per_level_above_first,
        } = self;
        quote! { #base + #per_level_above_first * (level - 1) as f32 }
    }
}

#[enum_dispatch]
pub(super) trait CompileLevelBasedFunction {
    fn compile(&self) -> TokenStream;
}

#[enum_dispatch]
pub(super) trait CompileNumberProvider {
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
#[enum_dispatch(CompileNumberProvider)]
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
#[enum_dispatch(CompileNumberProvider)]
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
