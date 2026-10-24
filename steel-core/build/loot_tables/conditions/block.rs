use super::super::{
    Compiler, TokenStream, ident,
    model::{CompileCondition, ItemPredicate, StateProperties},
    quote,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurvivesExplosion {}

impl CompileCondition for SurvivesExplosion {
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        quote! { ctx.explosion_radius.is_none_or(|radius| ctx.rng.next_f32() <= 1.0 / radius) }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchBlock {
    // TODO: Support the full BlockPredicate codec, including holder-set lists,
    // tags, ranged state properties, and block-entity NBT/component predicates.
    pub blocks: String,
    #[serde(default)]
    pub state: StateProperties,
}

impl CompileCondition for MatchBlock {
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        let Self { blocks, state, .. } = self;
        let block = ident(blocks);
        let properties = state
            .iter()
            .map(|(name, expected)| {
                quote! { state.get_property_str(#name).is_some_and(|value| value == #expected) }
            })
            .collect::<Vec<_>>();
        quote! { ctx.block_state.is_some_and(|state| state.get_block() == &steel_registry::vanilla_blocks::#block #(&& #properties)*) }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchTool {
    // TODO: Accept an absent predicate while retaining the required tool check.
    pub predicate: ItemPredicate,
}

impl CompileCondition for MatchTool {
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        let Self { predicate, .. } = self;
        let predicate = Compiler::item_predicate(predicate);
        quote! { ctx.tool.is_some_and(|item| #predicate) }
    }
}
