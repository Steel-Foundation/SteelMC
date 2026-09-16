use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use steel_utils::Identifier;

#[derive(Deserialize)]
#[expect(dead_code)]
pub(crate) struct CriterionJson {
    trigger: Identifier,
    #[serde(default)]
    conditions: Value,
}

impl ToTokens for CriterionJson {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(quote! {
            Box::new(Criterion::<ImpossibleInstance, ImpossibleTrigger>::default()) as Box<dyn AnyCriterion> // TODO when predicate are implemented
        });
    }
}

pub(crate) fn parse_criteria(list: &BTreeMap<String, CriterionJson>) -> TokenStream {
    let criteria = list.iter().map(|(k, v)| {
        quote! { (#k.to_owned(), #v) }
    });
    quote! {
        BTreeMap::from([#(#criteria),*])
    }
}