//! Generates table identities and categorized access from the compiler's inputs.
use std::collections::BTreeMap;

use heck::{ToSnakeCase, ToUpperCamelCase};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

use super::{ident, identifier, model::LootTable};

struct LootTableData<'a> {
    key: &'a str,
    const_ident: Ident,
}

pub(super) fn build(inputs: &[(String, LootTable)]) -> TokenStream {
    let mut tables = Vec::new();
    let mut stream = quote! {
        use crate::loot::{LootTable, LootTableRef, LootTableRegistry};
        use steel_utils::Identifier;
    };
    for (key, table) in inputs {
        let const_ident = ident(key);
        let doc = format!("`{key}`");
        let param_set = identifier(table.param_set.as_deref().unwrap_or("minecraft:generic"));
        let random_sequence = table.random_sequence.as_deref().map_or_else(
            || quote! { None },
            |value| {
                let value = identifier(value);
                quote! { Some(#value) }
            },
        );
        stream.extend(quote! {
            #[doc = #doc]
            pub static #const_ident: LootTable = LootTable {
                key: Identifier::vanilla_static(#key),
                param_set: #param_set,
                random_sequence: #random_sequence,
            };
        });
        tables.push(LootTableData { key, const_ident });
    }
    let identities = tables.iter().map(|table| &table.const_ident);
    stream.extend(quote! {
        const TABLES: &[LootTableRef] = &[#(&#identities),*];

        /// Registers the generated vanilla table identities in stable order.
        pub fn register_loot_tables(registry: &mut LootTableRegistry) {
            for &table in TABLES {
                registry.register(table);
            }
        }
    });
    stream.extend(accessors(&tables));
    stream
}

fn accessors(tables: &[LootTableData<'_>]) -> TokenStream {
    let mut categories: BTreeMap<&str, Vec<(&LootTableData<'_>, Ident)>> = BTreeMap::new();
    for table in tables {
        let category = table.key.split('/').next().unwrap_or("other");
        let field_name = table.key.split('/').skip(1).collect::<Vec<_>>().join("_");
        let field_name = if field_name.is_empty() {
            table.key
        } else {
            &field_name
        };
        let field = Ident::new(&field_name.to_snake_case(), Span::call_site());
        categories.entry(category).or_default().push((table, field));
    }

    let mut stream = TokenStream::new();
    let mut category_fields = Vec::new();
    let mut category_inits = Vec::new();
    for (category, tables) in categories {
        let name = Ident::new(
            &format!("{}LootTables", category.to_upper_camel_case()),
            Span::call_site(),
        );
        let category_field = Ident::new(&category.to_snake_case(), Span::call_site());
        let fields = tables.iter().map(|(table, field)| {
            let doc = format!("`{}`", table.key);
            quote! { #[doc = #doc] pub #field: LootTableRef, }
        });
        let inits = tables.iter().map(|(table, field)| {
            let const_ident = &table.const_ident;
            quote! { #field: &#const_ident, }
        });
        stream.extend(quote! {
            #[doc = concat!("Vanilla loot tables in the `", #category, "` category.")]
            pub struct #name { #(#fields)* }

            impl #name {
                /// Creates categorized references to the generated vanilla tables.
                #[must_use]
                pub const fn new() -> Self { Self { #(#inits)* } }
            }

            impl Default for #name {
                fn default() -> Self { Self::new() }
            }
        });
        category_fields.push(quote! {
            #[doc = concat!("`", #category, "`")]
            pub #category_field: #name,
        });
        category_inits.push(quote! { #category_field: #name::new(), });
    }
    stream.extend(quote! {
        /// Categorized references to the generated vanilla loot tables.
        pub struct LootTables { #(#category_fields)* }

        impl LootTables {
            /// Creates references to every vanilla loot-table category.
            #[must_use]
            pub const fn new() -> Self { Self { #(#category_inits)* } }
        }

        impl Default for LootTables {
            fn default() -> Self { Self::new() }
        }

        /// Vanilla loot tables grouped by category.
        pub static LOOT_TABLES: LootTables = LootTables::new();
    });
    stream
}
