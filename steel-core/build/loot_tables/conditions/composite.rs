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
        let terms = compiler.conditions(&self.terms);
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
        let terms = compiler.conditions(&self.terms);
        Compiler::all(&terms)
    }
}

impl Compiler {
    fn conditions(&mut self, terms: &[Holder<Condition>]) -> Vec<TokenStream> {
        terms
            .iter()
            .map(|term| self.condition(Some(term)))
            .collect()
    }
}
