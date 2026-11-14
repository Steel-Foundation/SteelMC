use super::super::{
    Compiler,
    model::{CompileEntry, Condition, Holder, LootPoolEntry, Modifier},
};
use super::EntryExpansion;
use proc_macro2::TokenStream;
use quote::quote;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlternativesEntry {
    pub children: Vec<LootPoolEntry>,
    pub condition: Option<Holder<Condition>>,
    pub modifier: Option<Modifier>,
}

impl CompileEntry for AlternativesEntry {
    fn compile_expansion(&self, compiler: &mut Compiler) -> EntryExpansion {
        EntryExpansion::Expression(Compiler::any(&compiler.expressions(&self.children)))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceEntry {
    pub children: Vec<LootPoolEntry>,
    pub condition: Option<Holder<Condition>>,
    pub modifier: Option<Modifier>,
}

impl CompileEntry for SequenceEntry {
    fn compile_expansion(&self, compiler: &mut Compiler) -> EntryExpansion {
        EntryExpansion::Expression(Compiler::all(&compiler.expressions(&self.children)))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupEntry {
    pub children: Vec<LootPoolEntry>,
    pub condition: Option<Holder<Condition>>,
    pub modifier: Option<Modifier>,
}

impl CompileEntry for GroupEntry {
    fn compile_expansion(&self, compiler: &mut Compiler) -> EntryExpansion {
        if let [child] = self.children.as_slice() {
            return compiler.entry(child);
        }
        let children = self
            .children
            .iter()
            .map(|child| compiler.entry(child).into_statements())
            .collect::<Vec<_>>();
        EntryExpansion::Statements(quote! { #(#children)* })
    }
}

impl Compiler {
    fn expressions(&mut self, children: &[LootPoolEntry]) -> Vec<TokenStream> {
        children
            .iter()
            .map(|child| self.entry(child).into_expression())
            .collect()
    }
}
