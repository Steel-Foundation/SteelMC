use super::super::{
    Compiler, TokenStream, ident, identifier,
    model::{CompileModifier, Condition, ContextIntProvider, Holder, HolderSet},
    quote,
};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetInstrument {
    pub options: HolderSet,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for SetInstrument {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        let Self { options, .. } = self;
        let options = match options {
            HolderSet::Single(key) if key.starts_with('#') => {
                let tag = identifier(&key[1..]);
                quote! {{
                    let Some(instruments) = REGISTRY.instruments.get_tag(&#tag) else {
                        return ctx.unsupported("missing instrument tag");
                    };
                    instruments
                }}
            }
            HolderSet::Single(key) => {
                let key = ident(key);
                quote! { vec![&steel_registry::vanilla_instruments::#key] }
            }
            HolderSet::List(keys) => {
                let instruments = keys.iter().map(|key| {
                    let key = ident(key);
                    quote! { &steel_registry::vanilla_instruments::#key }
                });
                quote! { vec![#(#instruments),*] }
            }
        };
        quote! { let options = #options;
            if !options.is_empty() { let index = ctx.rng.next_i32_bounded(options.len() as i32) as usize;
                item.set(steel_registry::data_components::vanilla_components::INSTRUMENT,
                    steel_registry::data_components::InstrumentComponent::new(steel_registry::RegistryHolder::reference(options[index])));
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetEnchantments {
    // TODO: Default an omitted enchantments map to an empty map, as vanilla does.
    pub enchantments: BTreeMap<String, ContextIntProvider>,
    #[serde(default)]
    pub add: bool,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for SetEnchantments {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, compiler: &mut Compiler) -> TokenStream {
        let Self {
            enchantments, add, ..
        } = self;
        let enchantments = enchantments.iter().map(|(key, value)| {
                    let key = identifier(key);
                    let level = compiler.int_provider(value);
                    quote! { let level = #level; crate::loot::modifiers::set_enchantment(item, #key, level, #add); }
                }).collect::<Vec<_>>();
        quote! {
            if item.is(&vanilla_items::BOOK) { item.set_item(&vanilla_items::ENCHANTED_BOOK.key); }
            if item.get_enchantments_for_crafting().is_some() { #(#enchantments)* }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopyComponents {
    pub source: String,
    pub include: Option<Vec<String>>,
    pub exclude: Option<Vec<String>>,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for CopyComponents {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        let Self {
            source,
            include,
            exclude,
            ..
        } = self;
        let only_name = include
            .as_ref()
            .is_some_and(|values| values.iter().all(|value| value == "minecraft:custom_name"));
        if source != "block_entity" || !only_name {
            // TODO: Collect and copy all supported source components with vanilla
            // include/exclude filtering, including decorated-pot decorations.
            Compiler::unsupported_modifier("minecraft:copy_components")
        } else if include.as_ref().is_some_and(Vec::is_empty)
            || exclude
                .as_ref()
                .is_some_and(|values| values.iter().any(|value| value == "minecraft:custom_name"))
        {
            quote! {}
        } else {
            quote! { crate::loot::modifiers::copy_block_entity_name(ctx, item)?; }
        }
    }
}
