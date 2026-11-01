use super::super::{
    Compiler, TokenStream, identifier,
    model::{Bounds, CompileModifier, Condition, ContextFloatProvider, ContextIntProvider, Holder},
    quote,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetCount {
    pub count: ContextIntProvider,
    #[serde(default)]
    pub add: bool,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for SetCount {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, compiler: &mut Compiler) -> TokenStream {
        let Self { count, add, .. } = self;
        let count = compiler.int_provider(count);
        quote! { let count = #count; item.set_count(if #add { item.count().wrapping_add(count) } else { count }); }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnchantedCountIncrease {
    pub enchantment: String,
    pub count: ContextFloatProvider,
    #[serde(default)]
    pub limit: i32,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for EnchantedCountIncrease {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, compiler: &mut Compiler) -> TokenStream {
        let Self {
            enchantment,
            count,
            limit,
            ..
        } = self;
        let enchantment = identifier(enchantment);
        let count = compiler.float_provider(count);
        let clamp = (*limit > 0).then(|| quote! { item.set_count(item.count().min(#limit)); });
        quote! { let level = ctx.attacker_enchantment_level(&#enchantment);
            if level > 0 { let count = #count; let bonus = crate::loot::modifiers::round_count(level as f32 * count);
                item.set_count(item.count().wrapping_add(bonus));
                #clamp
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitCount {
    pub limit: Bounds<ContextIntProvider>,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for LimitCount {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, compiler: &mut Compiler) -> TokenStream {
        let Self { limit, .. } = self;
        let min = limit.min.as_ref().map(|value| {
            let value = compiler.int_provider(value);
            quote! { let min = #value; item.set_count(item.count().max(min)); }
        });
        let max = limit.max.as_ref().map(|value| {
            let value = compiler.int_provider(value);
            quote! { let max = #value; item.set_count(item.count().min(max)); }
        });
        quote! { #min #max }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetOminousBottleAmplifier {
    pub amplifier: ContextIntProvider,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for SetOminousBottleAmplifier {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, compiler: &mut Compiler) -> TokenStream {
        let Self { amplifier, .. } = self;
        let amplifier = compiler.int_provider(amplifier);
        quote! { let amplifier = #amplifier;
        item.set_ominous_bottle_amplifier(amplifier.clamp(steel_registry::data_components::OminousBottleAmplifier::MIN_AMPLIFIER,
            steel_registry::data_components::OminousBottleAmplifier::MAX_AMPLIFIER)); }
    }
}
