use super::{
    Compiler, TokenStream,
    model::{CompileCondition, Condition, Holder},
    quote,
};
mod block;
mod composite;
mod entity;
mod random;

pub use block::{MatchBlock, MatchTool, SurvivesExplosion};
pub use composite::{AllOf, AnyOf, Inverted};
pub use entity::{DamageSourceProperties, EntityProperties, KilledByPlayer, LocationCheck};
pub use random::{RandomChance, RandomChanceWithEnchantedBonus, TableBonus};
impl Compiler {
    pub(super) fn condition(&mut self, condition: Option<&Holder<Condition>>) -> TokenStream {
        match condition {
            None => quote! { true },
            Some(Holder::Reference(key)) => {
                self.with_reference("predicate", key, |compiler, condition| {
                    compiler.condition(Some(condition))
                })
            }
            Some(Holder::Inline(condition)) => condition.compile(self),
        }
    }
}
