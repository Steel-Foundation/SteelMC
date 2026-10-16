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
    fn decorations(&self) -> (Option<&Holder<Condition>>, Option<&Modifier>) {
        (self.condition.as_ref(), self.modifier.as_ref())
    }
    fn compile_expansion(&self, _compiler: &mut Compiler) -> EntryExpansion {
        let Self {
            name,
            weight,
            quality,
            ..
        } = self;
        let item = ident(name);
        Compiler::weighted_candidate(
            *weight,
            *quality,
            quote! { |ctx, output| output(ctx, ItemStack::new(&vanilla_items::#item)) },
        )
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
    fn decorations(&self) -> (Option<&Holder<Condition>>, Option<&Modifier>) {
        (self.condition.as_ref(), self.modifier.as_ref())
    }
    fn compile_expansion(&self, _compiler: &mut Compiler) -> EntryExpansion {
        let Self {
            name,
            weight,
            quality,
            ..
        } = self;
        let operation = format!("dynamic:{name}");
        // TODO: Emit context-provided named dynamic drops when selected, including
        // decorated-pot sherds, instead of rejecting every dynamic entry.
        Compiler::weighted_candidate(
            *weight,
            *quality,
            quote! { |ctx, _| Err(ctx.unsupported(#operation)) },
        )
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
    fn decorations(&self) -> (Option<&Holder<Condition>>, Option<&Modifier>) {
        (self.condition.as_ref(), self.modifier.as_ref())
    }
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
                quote! { vanilla_loot_tables::#table.emit_raw(ctx, output) }
            }
            Holder::Inline(table) => {
                let function = compiler.table(table);
                quote! { #function(ctx, output) }
            }
        };
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
    fn decorations(&self) -> (Option<&Holder<Condition>>, Option<&Modifier>) {
        (self.condition.as_ref(), self.modifier.as_ref())
    }
    fn compile_expansion(&self, _compiler: &mut Compiler) -> EntryExpansion {
        Compiler::weighted_candidate(self.weight, self.quality, quote! { |_, _| Ok(()) })
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
    fn decorations(&self) -> (Option<&Holder<Condition>>, Option<&Modifier>) {
        (self.condition.as_ref(), self.modifier.as_ref())
    }
    fn compile_expansion(&self, _compiler: &mut Compiler) -> EntryExpansion {
        let Self {
            items,
            expand,
            weight,
            quality,
            ..
        } = self;
        let tag = identifier(
            items
                .strip_prefix('#')
                .unwrap_or_else(|| panic!("Expected item tag: {items}")),
        );
        if *expand {
            EntryExpansion::Statements(quote! {{
                let items = REGISTRY.items.get_tag(&#tag).ok_or_else(|| ctx.unsupported("missing item tag"))?;
                for item in items {
                    candidates.push(LootCandidate::new(#weight, #quality, ctx.luck, move |ctx, output| output(ctx, ItemStack::new(item))));
                }
            }})
        } else {
            Compiler::weighted_candidate(
                *weight,
                *quality,
                quote! { |ctx, output| {
                        let items = REGISTRY.items.get_tag(&#tag).ok_or_else(|| ctx.unsupported("missing item tag"))?;
                        for item in items { output(ctx, ItemStack::new(item))?; }
                        Ok(())
                    }
                },
            )
        }
    }
}
