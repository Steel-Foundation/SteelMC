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
mod location_test_tables;
mod metadata;
mod model;
mod modifiers;
mod name_test_tables;
mod numbers;
mod predicates;
mod requirements;
mod test_tables;

const INPUT_ROOT: &str = "../steel-utils/build_assets/builtin_datapacks/minecraft";

/// Build-time compiler state. No JSON or executable AST is shipped to the server.
#[derive(Default)]
struct Compiler {
    resumable: bool,
    definitions: TokenStream,
    next_id: usize,
    reference_stack: BTreeSet<PathBuf>,
}

impl Compiler {
    fn call(&self, expression: TokenStream) -> TokenStream {
        if self.resumable {
            quote! { #expression.await }
        } else {
            expression
        }
    }

    fn continuation(&self, body: TokenStream) -> TokenStream {
        if self.resumable {
            quote! { Box::pin(async move { #body }) }
        } else {
            quote! {{ #body }}
        }
    }

    fn emit(&self, item: TokenStream) -> TokenStream {
        if self.resumable {
            quote! { output.emit(ctx, #item).await }
        } else {
            quote! { output(ctx, #item) }
        }
    }

    fn output_function(&mut self, name: &Ident, body: TokenStream) {
        let definition = if self.resumable {
            quote! { pub(super) fn #name<'a>(ctx: &'a mut LootContext<'_>, output: &'a mut dyn LootOutput) -> LootFuture<'a> { Box::pin(async move { #body }) } }
        } else {
            quote! { fn #name(ctx: &mut LootContext<'_>, output: &mut LootOutput<'_>) -> OperationResult<()> { #body } }
        };
        self.definitions.extend(definition);
    }

    fn decorated_output(&self, modifier: &Ident, body: TokenStream) -> TokenStream {
        if self.resumable {
            quote! {
                let modifiers: [LootModifier; 1] = [#modifier];
                let mut modified = ModifiedLootOutput::new(output, &modifiers);
                let output: &mut dyn LootOutput = &mut modified;
                #body
            }
        } else {
            quote! { with_modifier(ctx, output, #modifier, |ctx, output| { #body }) }
        }
    }

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
        let path = Path::new(INPUT_ROOT)
            .join(directory)
            .join(format!("{path}.json"));
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
        let calls = pools
            .iter()
            .map(|pool| self.call(quote! { #pool(ctx, output) }))
            .collect::<Vec<_>>();
        let body = self.decorated_output(
            &modifier,
            quote! {
                let _ = (&ctx, &output);
                #(#calls?;)*
                Ok(())
            },
        );
        self.output_function(&name, body);
        name
    }
}

pub(crate) fn build() -> TokenStream {
    let root = Path::new(INPUT_ROOT).join("loot_table");
    println!("cargo:rerun-if-changed={INPUT_ROOT}");
    let mut paths = Vec::new();
    collect(&root, &mut paths);
    paths.sort();
    let inputs = paths
        .iter()
        .map(|path| {
            let key = path
                .strip_prefix(&root)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            (key, Compiler::read_input::<LootTable>(path))
        })
        .collect::<Vec<_>>();
    let mut direct = Compiler::default();
    let mut resumable = Compiler {
        resumable: true,
        ..Compiler::default()
    };
    let mut direct_registrations = Vec::new();
    let mut async_registrations = Vec::new();
    for ((key, table), path) in inputs.iter().zip(paths.iter()) {
        let identity = ident(key);
        let (direct_fn, async_fn) = catch_unwind(AssertUnwindSafe(|| {
            let direct_fn =
                (!requirements::requires_resumption(table, &inputs)).then(|| direct.table(table));
            (direct_fn, resumable.table(table))
        }))
        .unwrap_or_else(|error| {
            eprintln!("While compiling loot table {}", path.display());
            resume_unwind(error)
        });
        if let Some(function) = direct_fn {
            direct_registrations
                .push(quote! { registry.register_pair(&vanilla_loot_tables::#identity, #function, super::resumable::#async_fn); });
        } else {
            async_registrations.push(
                quote! { registry.register_async(&vanilla_loot_tables::#identity, #async_fn); },
            );
        }
    }
    let metadata = metadata::build(&inputs);
    let direct_definitions = direct.definitions;
    let async_definitions = resumable.definitions;
    let test_tables = test_tables::build(&inputs);
    quote! {
        use steel_registry::{REGISTRY, RegistryExt, TaggedRegistryExt, item_stack::ItemStack, vanilla_items};
        use steel_registry::blocks::block_state_ext::BlockStateExt;
        use super::{LootContext, LootContextEntity, LootTableBehaviorRegistry};
        /// Built-in loot-table identities and categorized access.
        pub mod vanilla_loot_tables { #metadata }
        mod direct {
            use super::{LootContext, ItemStack, vanilla_items, RegistryExt, BlockStateExt, REGISTRY, TaggedRegistryExt, LootContextEntity, vanilla_loot_tables, LootTableBehaviorRegistry};
            use crate::loot::{LootCandidate, LootOutput, OperationResult, with_modifier};
            const fn identity(_: &mut LootContext<'_>, _: &mut ItemStack) -> OperationResult<()> { Ok(()) }
            #direct_definitions
            pub(super) fn register(registry: &mut LootTableBehaviorRegistry) { #(#direct_registrations)* }
        }
        mod resumable {
            use super::{LootContext, ItemStack, vanilla_items, RegistryExt, BlockStateExt, REGISTRY, TaggedRegistryExt, LootContextEntity, vanilla_loot_tables, LootTableBehaviorRegistry};
            use crate::loot::{AsyncLootCandidate as LootCandidate, LootAsyncModifier as LootModifier,
                LootAsyncOutput as LootOutput, ModifiedAsyncLootOutput as ModifiedLootOutput, LootFuture};
            fn identity<'a>(_: &'a mut LootContext<'_>, _: &'a mut ItemStack) -> LootFuture<'a> { Box::pin(async { Ok(()) }) }
            #async_definitions
            pub(super) fn register(registry: &mut LootTableBehaviorRegistry) { #(#async_registrations)* }
        }
        pub(super) fn register(registry: &mut LootTableBehaviorRegistry) {
            direct::register(registry);
            resumable::register(registry);
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
