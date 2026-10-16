use super::{
    Compiler, Ident, TokenStream,
    model::{CompileEntry, LootPool, LootPoolEntry},
    quote,
};

mod composite;
mod single;

pub use composite::{AlternativesEntry, GroupEntry, SequenceEntry};
pub use single::{DynamicEntry, EmptyEntry, ItemEntry, LootTableEntry, TagEntry};

/// Inline expansion code. Statements succeed unconditionally; expressions report
/// success for alternatives and sequences without discarding their side effects.
pub(super) enum EntryExpansion {
    Statements(TokenStream),
    Expression(TokenStream),
}

impl EntryExpansion {
    fn into_expression(self) -> TokenStream {
        match self {
            Self::Statements(statements) => quote! {{ #statements true }},
            Self::Expression(expression) => expression,
        }
    }

    fn into_statements(self) -> TokenStream {
        match self {
            Self::Statements(statements) => statements,
            Self::Expression(expression) => quote! { let _ = #expression; },
        }
    }

    fn with_modifier(self, modifier: &Ident) -> Self {
        let decorate = quote! {
            for candidate in &mut candidates[start..] { candidate.add_modifier(#modifier); }
        };
        match self {
            Self::Statements(statements) => Self::Statements(quote! {{
                let start = candidates.len();
                #statements
                #decorate
            }}),
            Self::Expression(expression) => Self::Expression(quote! {{
                let start = candidates.len();
                let expanded = #expression;
                #decorate
                expanded
            }}),
        }
    }
}

impl Compiler {
    pub(super) fn pool(&mut self, pool: &LootPool) -> Ident {
        let condition = self.condition(pool.condition.as_ref());
        let guard = pool
            .condition
            .as_ref()
            .map(|_| quote! { let passes = #condition; if !passes { return Ok(()); } });
        let rolls = self.int_provider(&pool.rolls);
        let bonus = pool
            .bonus_rolls
            .as_ref()
            .map_or_else(|| quote! { 0.0_f32 }, |value| self.float_provider(value));
        let entries = pool
            .entries
            .iter()
            .map(|entry| self.entry(entry).into_statements())
            .collect::<Vec<_>>();
        let modifier = self.modifier(pool.modifier.as_ref());
        let name = self.name("pool");
        self.definitions.extend(quote! {
            fn #name(ctx: &mut LootContext<'_>, output: &mut LootOutput<'_>) -> LootResult<()> {
                #guard
                with_modifier(ctx, output, #modifier, |ctx, output| {
                    let rolls = #rolls;
                    let bonus = #bonus;
                    let rolls = rolls.wrapping_add((bonus * ctx.luck).floor() as i32);
                    for _ in 0..rolls {
                        let mut candidates = Vec::new();
                        #(#entries)*
                        LootCandidate::select(&candidates, ctx, output)?;
                    }
                    Ok(())
                })
            }
        });
        name
    }

    fn entry(&mut self, entry: &LootPoolEntry) -> EntryExpansion {
        let (condition_input, modifier_input) = entry.decorations();
        let mut expansion = entry.compile_expansion(self);
        if let Some(modifier) = modifier_input {
            let modifier = self.modifier(Some(modifier));
            expansion = expansion.with_modifier(&modifier);
        }
        if let Some(condition) = condition_input {
            let condition = self.condition(Some(condition));
            let expression = expansion.into_expression();
            expansion = EntryExpansion::Expression(quote! {{
                let passes = #condition;
                if passes { #expression } else { false }
            }});
        }
        expansion
    }

    fn weighted_candidate(weight: i32, quality: i32, create: TokenStream) -> EntryExpansion {
        EntryExpansion::Statements(quote! {
            candidates.push(LootCandidate::new(#weight, #quality, ctx.luck, #create));
        })
    }
}
