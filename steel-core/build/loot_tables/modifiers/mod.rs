use super::{
    Compiler, Ident, TokenStream,
    model::{CompileModifier, Modifier, ModifierFunction},
    quote,
};

mod bonus;
mod components;
mod control;
mod count;
mod deferred;

pub use bonus::{ApplyBonus, BinomialWithBonusCount, OreDrops, UniformBonusCount};
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
