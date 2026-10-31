//! Transpiles extracted vanilla loot tables into Rust control flow.
use std::{
    collections::BTreeSet,
    fs,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    path::{Path, PathBuf},
};

use heck::ToShoutySnakeCase;
use model::LootTable;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote};
use serde::de::DeserializeOwned;

mod conditions;
mod entries;
mod metadata;
mod model;
mod modifiers;
mod numbers;
mod predicates;
mod test_tables;

/// Build-time compiler state. No JSON or executable AST is shipped to the server.
struct Compiler {
    input_root: PathBuf,
    definitions: TokenStream,
    next_id: usize,
    reference_stack: BTreeSet<PathBuf>,
}

impl Default for Compiler {
    fn default() -> Self {
        Self {
            input_root: PathBuf::from("../steel-utils/build_assets/builtin_datapacks/minecraft"),
            definitions: TokenStream::new(),
            next_id: 0,
            reference_stack: BTreeSet::new(),
        }
    }
}

impl Compiler {
    fn read_input<T: DeserializeOwned>(path: &Path) -> T {
        let source = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        serde_json::from_str(&source).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn with_reference<T: DeserializeOwned, R>(
        &mut self,
        directory: &str,
        key: &str,
        compile: impl FnOnce(&mut Self, &T) -> R,
    ) -> R {
        let path = key
            .strip_prefix("minecraft:")
            .unwrap_or_else(|| panic!("Non-vanilla {directory} reference {key}"));
        let path = self.input_root.join(directory).join(format!("{path}.json"));
        assert!(
            self.reference_stack.insert(path.clone()),
            "Recursive reference {}",
            path.display()
        );
        let input = Self::read_input(&path);
        let result = compile(self, &input);
        self.reference_stack.remove(&path);
        result
    }

    fn name(&mut self, prefix: &str) -> Ident {
        let id = self.next_id;
        self.next_id += 1;
        format_ident!("{prefix}_{id}")
    }

    fn table(&mut self, table: &LootTable) -> Ident {
        let pools = table
            .pools
            .iter()
            .map(|pool| self.pool(pool))
            .collect::<Vec<_>>();
        let modifier = self.modifier(table.modifier.as_ref());
        let name = self.name("table");
        self.definitions.extend(quote! {
            fn #name(ctx: &mut LootContext<'_>, output: &mut LootOutput<'_>) -> LootResult<()> {
                with_modifier(ctx, output, #modifier, |ctx, output| {
                    let _ = (&ctx, &output);
                    #(#pools(ctx, output)?;)*
                    Ok(())
                })
            }
        });
        name
    }
}

pub(crate) fn build() -> TokenStream {
    let mut compiler = Compiler::default();
    let root = compiler.input_root.join("loot_table");
    // Watch registry directories that a later extraction may create as well.
    println!("cargo:rerun-if-changed={}", compiler.input_root.display());
    let mut paths = Vec::new();
    collect(&root, &mut paths);
    paths.sort();
    let mut inputs = Vec::new();
    let mut registrations = Vec::new();
    for path in paths {
        let key = path
            .strip_prefix(&root)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        let table = Compiler::read_input::<LootTable>(&path);
        // Include the file in compiler diagnostics, including validation failures.
        let function =
            catch_unwind(AssertUnwindSafe(|| compiler.table(&table))).unwrap_or_else(|error| {
                eprintln!("While compiling loot table {}", path.display());
                resume_unwind(error)
            });
        let identity = ident(&key);
        registrations.push(quote! { (&vanilla_loot_tables::#identity, #function) });
        inputs.push((key, table));
    }
    let metadata = metadata::build(&inputs);
    let definitions = compiler.definitions;
    let test_tables = test_tables::build();
    quote! {
        use steel_registry::{REGISTRY, RegistryExt, TaggedRegistryExt, item_stack::ItemStack, vanilla_items};
        use steel_utils::random::Random;
        use steel_registry::blocks::block_state_ext::BlockStateExt;
        use super::{LootCandidate, LootContext, LootContextEntity, LootOutput, LootResult, LootTableBehaviorRegistry, LootTableFunction, LootTableRef, with_modifier};
        /// Built-in loot-table identities and categorized access.
        pub mod vanilla_loot_tables {
            #metadata
        }
        const fn identity(_: &mut LootContext<'_>, _: &mut ItemStack) -> LootResult<()> { Ok(()) }
        #definitions
        const TABLE_FUNCTIONS: &[(LootTableRef, LootTableFunction)] = &[#(#registrations),*];
        pub(super) fn register(registry: &mut LootTableBehaviorRegistry) {
            for &(table, function) in TABLE_FUNCTIONS {
                registry.register(table, function);
            }
        }
        #test_tables
    }
}

fn collect(root: &Path, paths: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).unwrap_or_else(|e| panic!("{}: {e}", root.display())) {
        let path = entry
            .unwrap_or_else(|e| panic!("{}: {e}", root.display()))
            .path();
        if path.is_dir() {
            collect(&path, paths);
        } else if path.extension().is_some_and(|ext| ext == "json") {
            paths.push(path);
        }
    }
}

fn ident(key: &str) -> Ident {
    let key = key.strip_prefix("minecraft:").unwrap_or(key);
    Ident::new(
        &key.replace(['/', '.'], "_").to_shouty_snake_case(),
        Span::call_site(),
    )
}

fn identifier(key: &str) -> TokenStream {
    let (namespace, path) = key.split_once(':').unwrap_or(("minecraft", key));
    quote! { steel_utils::Identifier::new_static(#namespace, #path) }
}
