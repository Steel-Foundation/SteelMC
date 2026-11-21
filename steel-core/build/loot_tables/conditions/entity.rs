use super::super::{
    Compiler, TokenStream,
    model::{CompileCondition, DamageSourcePredicate, EntityPredicate, EntityTarget},
    quote,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KilledByPlayer {}

impl CompileCondition for KilledByPlayer {
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        quote! { ctx.killed_by_player }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityProperties {
    // TODO: Accept an absent predicate with vanilla's unconditional success.
    pub entity: EntityTarget,
    pub predicate: EntityPredicate,
}

impl CompileCondition for EntityProperties {
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        let Self {
            entity, predicate, ..
        } = self;
        let target = match entity {
            EntityTarget::This => quote! { LootContextEntity::This },
            EntityTarget::Attacker => quote! { LootContextEntity::Killer },
            EntityTarget::DirectAttacker => quote! { LootContextEntity::DirectKiller },
            EntityTarget::LastDamagePlayer => quote! { LootContextEntity::KillerPlayer },
            EntityTarget::InteractingEntity => quote! { LootContextEntity::Interacting },
        };
        let predicate = Compiler::entity_predicate(predicate);
        quote! { match ctx.get_entity(#target) { Some(entity) => { let _ = &entity; #predicate }, None => false } }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DamageSourceProperties {
    // TODO: Accept an absent predicate while requiring origin and damage source.
    pub predicate: Box<DamageSourcePredicate>,
}

impl CompileCondition for DamageSourceProperties {
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        let Self { predicate, .. } = self;
        let predicate = Compiler::damage_source_predicate(predicate);
        quote! { ctx.origin.is_some() && (#predicate) }
    }
}
