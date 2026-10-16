use super::{
    Compiler, TokenStream,
    model::{
        CompileFloatProvider, CompileIntProvider, CompileLevelBasedFunction, ContextFloatProvider,
        ContextIntProvider, NumberProvider,
    },
    quote,
};
use serde::Deserialize;

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

impl CompileIntProvider for IntConstant {
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

impl CompileIntProvider for IntUniform {
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

impl CompileIntProvider for IntBinomial {
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

impl CompileFloatProvider for FloatConstant {
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

impl CompileFloatProvider for FloatUniform {
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
