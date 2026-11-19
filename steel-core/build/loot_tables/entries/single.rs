use super::super::{
    Compiler, ident, identifier,
    model::{CompileEntry, Condition, Holder, LootTable, Modifier, one},
    quote,
};
use super::EntryExpansion;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemEntry {
    pub name: String,
    #[serde(default = "one")]
    pub weight: i32,
    #[serde(default)]
    pub quality: i32,
    pub condition: Option<Holder<Condition>>,
    pub modifier: Option<Modifier>,
}

impl CompileEntry for ItemEntry {
    fn compile_expansion(&self, compiler: &mut Compiler) -> EntryExpansion {
        let Self {
            name,
            weight,
            quality,
            ..
        } = self;
        let item = ident(name);
        let emit = compiler.emit(quote! { ItemStack::new(&vanilla_items::#item) });
        let create = compiler.continuation(emit);
        Compiler::weighted_candidate(*weight, *quality, quote! { |ctx, output| #create })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicEntry {
    pub name: String,
    #[serde(default = "one")]
    pub weight: i32,
    #[serde(default)]
    pub quality: i32,
    pub condition: Option<Holder<Condition>>,
    pub modifier: Option<Modifier>,
}

impl CompileEntry for DynamicEntry {
    fn compile_expansion(&self, compiler: &mut Compiler) -> EntryExpansion {
        let Self {
            name,
            weight,
            quality,
            ..
        } = self;
        let operation = format!(
            "minecraft:dynamic entry '{name}': context-provided dynamic drops are not implemented"
        );
        // TODO: Emit context-provided named dynamic drops when selected, including
        // decorated-pot sherds, instead of rejecting every dynamic entry.
        let create = compiler.continuation(quote! { ctx.unsupported(#operation) });
        Compiler::weighted_candidate(*weight, *quality, quote! { |ctx, _| #create })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LootTableEntry {
    pub value: Holder<LootTable>,
    #[serde(default = "one")]
    pub weight: i32,
    #[serde(default)]
    pub quality: i32,
    pub condition: Option<Holder<Condition>>,
    pub modifier: Option<Modifier>,
}

impl CompileEntry for LootTableEntry {
    fn compile_expansion(&self, compiler: &mut Compiler) -> EntryExpansion {
        let Self {
            value,
            weight,
            quality,
            ..
        } = self;
        let create = match value {
            Holder::Reference(key) => {
                let table = ident(key);
                if compiler.resumable {
                    quote! { vanilla_loot_tables::#table.emit_raw_async(ctx, output).await }
                } else {
                    quote! { vanilla_loot_tables::#table.emit_raw(ctx, output) }
                }
            }
            Holder::Inline(table) => {
                let function = compiler.table(table);
                compiler.call(quote! { #function(ctx, output) })
            }
        };
        let create = compiler.continuation(create);
        Compiler::weighted_candidate(*weight, *quality, quote! { |ctx, output| #create })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyEntry {
    #[serde(default = "one")]
    pub weight: i32,
    #[serde(default)]
    pub quality: i32,
    pub condition: Option<Holder<Condition>>,
    pub modifier: Option<Modifier>,
}

impl CompileEntry for EmptyEntry {
    fn compile_expansion(&self, compiler: &mut Compiler) -> EntryExpansion {
        let create = compiler.continuation(quote! { Ok(()) });
        Compiler::weighted_candidate(self.weight, self.quality, quote! { |_, _| #create })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagEntry {
    // TODO: Support vanilla's direct item holder-set lists as well as named tags.
    pub items: String,
    #[serde(default)]
    pub expand: bool,
    #[serde(default = "one")]
    pub weight: i32,
    #[serde(default)]
    pub quality: i32,
    pub condition: Option<Holder<Condition>>,
    pub modifier: Option<Modifier>,
}

impl CompileEntry for TagEntry {
    fn compile_expansion(&self, compiler: &mut Compiler) -> EntryExpansion {
        let Self {
            items,
            expand,
            weight,
            quality,
            ..
        } = self;
        let Some(tag_name) = items.strip_prefix('#') else {
            panic!(
                "minecraft:tag entry has unsupported 'items' value {items:?}: expected a named item tag starting with '#'; direct item holder sets are not implemented"
            );
        };
        let tag = identifier(tag_name);
        let missing_tag = format!("minecraft:tag entry: item tag '{items}' is not registered");
        let emit = compiler.emit(quote! { ItemStack::new(item) });
        if *expand {
            let create = compiler.continuation(emit);
            EntryExpansion::Statements(quote! {{
                let Some(items) = REGISTRY.items.get_tag(&#tag) else { return ctx.unsupported(#missing_tag); };
                for item in items { candidates.push(LootCandidate::new(#weight, #quality, ctx.luck, move |ctx, output| #create)); }
            }})
        } else {
            let create = compiler.continuation(quote! {
                let Some(items) = REGISTRY.items.get_tag(&#tag) else { return ctx.unsupported(#missing_tag); };
                for item in items { #emit?; }
                Ok(())
            });
            Compiler::weighted_candidate(*weight, *quality, quote! { |ctx, output| #create })
        }
    }
}
