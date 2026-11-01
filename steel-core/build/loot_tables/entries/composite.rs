use super::super::{
    Compiler,
    model::{CompileEntry, Condition, Holder, LootPoolEntry, Modifier},
};
use super::EntryExpansion;
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
    fn decorations(&self) -> (Option<&Holder<Condition>>, Option<&Modifier>) {
        (self.condition.as_ref(), self.modifier.as_ref())
    }
    fn compile_expansion(&self, compiler: &mut Compiler) -> EntryExpansion {
        let terms = self
            .children
            .iter()
            .map(|child| compiler.entry(child).into_expression())
            .collect::<Vec<_>>();
        EntryExpansion::Expression(Compiler::any(&terms))
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
    fn decorations(&self) -> (Option<&Holder<Condition>>, Option<&Modifier>) {
        (self.condition.as_ref(), self.modifier.as_ref())
    }
    fn compile_expansion(&self, compiler: &mut Compiler) -> EntryExpansion {
        let terms = self
            .children
            .iter()
            .map(|child| compiler.entry(child).into_expression())
            .collect::<Vec<_>>();
        EntryExpansion::Expression(Compiler::all(&terms))
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
    fn decorations(&self) -> (Option<&Holder<Condition>>, Option<&Modifier>) {
        (self.condition.as_ref(), self.modifier.as_ref())
    }
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
