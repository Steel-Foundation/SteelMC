use super::super::{
    Compiler, TokenStream,
    model::{CompileModifier, Condition, Holder, ItemPredicate, Modifier, yes},
    quote,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplosionDecay {
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for ExplosionDecay {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        quote! {
            if let Some(radius) = ctx.explosion_radius {
                let mut count = 0;
                for _ in 0..item.count() { if ctx.rng.next_f32() <= 1.0 / radius { count += 1; } }
                item.set_count(count);
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Discard {
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for Discard {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        quote! { *item = ItemStack::empty(); }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FurnaceSmelt {
    #[serde(default = "yes")]
    pub use_input_count: bool,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for FurnaceSmelt {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        let Self {
            use_input_count, ..
        } = self;
        quote! { crate::loot::modifiers::smelt(item, #use_input_count); }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filtered {
    pub item_filter: ItemPredicate,
    pub on_pass: Option<Modifier>,
    pub on_fail: Option<Modifier>,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for Filtered {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, compiler: &mut Compiler) -> TokenStream {
        let Self {
            item_filter,
            on_pass,
            on_fail,
            ..
        } = self;
        let filter = Compiler::item_predicate(item_filter);
        let on_pass = compiler.modifier(on_pass.as_ref());
        let on_fail = compiler.modifier(on_fail.as_ref());
        quote! { if #filter { #on_pass(ctx, item)?; } else { #on_fail(ctx, item)?; } }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sequence {
    pub functions: Modifier,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for Sequence {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, compiler: &mut Compiler) -> TokenStream {
        let Self { functions, .. } = self;
        let functions = compiler.modifier(Some(functions));
        quote! { #functions(ctx, item)?; }
    }
}
