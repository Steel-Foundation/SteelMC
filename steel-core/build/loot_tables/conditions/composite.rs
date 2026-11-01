use super::super::{
    Compiler, TokenStream,
    model::{CompileCondition, Condition, Holder},
    quote,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inverted {
    pub term: Box<Holder<Condition>>,
}

impl CompileCondition for Inverted {
    fn compile(&self, compiler: &mut Compiler) -> TokenStream {
        let Self { term, .. } = self;
        let inner = compiler.condition(Some(term));
        quote! { !(#inner) }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnyOf {
    pub terms: Vec<Holder<Condition>>,
}

impl CompileCondition for AnyOf {
    fn compile(&self, compiler: &mut Compiler) -> TokenStream {
        let Self { terms, .. } = self;
        let terms = terms
            .iter()
            .map(|term| compiler.condition(Some(term)))
            .collect::<Vec<_>>();
        Compiler::any(&terms)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllOf {
    pub terms: Vec<Holder<Condition>>,
}

impl CompileCondition for AllOf {
    fn compile(&self, compiler: &mut Compiler) -> TokenStream {
        let Self { terms, .. } = self;
        let terms = terms
            .iter()
            .map(|term| compiler.condition(Some(term)))
            .collect::<Vec<_>>();
        Compiler::all(&terms)
    }
}
