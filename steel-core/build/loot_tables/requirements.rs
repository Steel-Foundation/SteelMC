//! Transitive analysis of tables that can suspend for world data.
use std::collections::{BTreeMap, BTreeSet};

use super::{
    Compiler,
    model::{Decorated, Holder, LootPoolEntry, LootTable, Modifier, ModifierFunction},
};

pub(super) fn requires_resumption(table: &LootTable, inputs: &[(String, LootTable)]) -> bool {
    let tables = inputs
        .iter()
        .map(|(key, table)| (key.as_str(), table))
        .collect();
    Requirements {
        tables,
        visiting: BTreeSet::new(),
        compiler: Compiler::default(),
    }
    .table(table)
}

struct Requirements<'a> {
    tables: BTreeMap<&'a str, &'a LootTable>,
    visiting: BTreeSet<String>,
    compiler: Compiler,
}

impl Requirements<'_> {
    fn table(&mut self, table: &LootTable) -> bool {
        self.modifier(table.modifier.as_ref())
            || table.pools.iter().any(|pool| {
                self.modifier(pool.modifier.as_ref())
                    || pool.entries.iter().any(|entry| self.entry(entry))
            })
    }

    fn modifier(&mut self, modifier: Option<&Modifier>) -> bool {
        modifier_requires_resumption(&mut self.compiler, modifier)
    }

    fn entry(&mut self, entry: &LootPoolEntry) -> bool {
        if self.modifier(entry.decorations().1) {
            return true;
        }
        match entry {
            LootPoolEntry::LootTable(entry) => match &entry.value {
                Holder::Inline(table) => self.table(table),
                Holder::Reference(key) => {
                    if !self.visiting.insert(key.to_owned()) {
                        return false;
                    }
                    let path = key
                        .strip_prefix("minecraft:")
                        .unwrap_or_else(|| panic!("Non-vanilla loot table {key}"));
                    let table = self
                        .tables
                        .get(path)
                        .copied()
                        .unwrap_or_else(|| panic!("Missing loot table {key}"));
                    let result = self.table(table);
                    self.visiting.remove(key);
                    result
                }
            },
            LootPoolEntry::Alternatives(entry) => {
                entry.children.iter().any(|entry| self.entry(entry))
            }
            LootPoolEntry::Group(entry) => entry.children.iter().any(|entry| self.entry(entry)),
            LootPoolEntry::Sequence(entry) => entry.children.iter().any(|entry| self.entry(entry)),
            _ => false,
        }
    }
}

fn modifier_requires_resumption(compiler: &mut Compiler, modifier: Option<&Modifier>) -> bool {
    match modifier {
        None => false,
        Some(Modifier::Sequence(modifiers)) => modifiers
            .iter()
            .any(|modifier| modifier_requires_resumption(compiler, Some(modifier))),
        Some(Modifier::Reference(key)) => {
            compiler.with_reference("item_modifier", key, |compiler, modifier| {
                modifier_requires_resumption(compiler, Some(modifier))
            })
        }
        Some(Modifier::Inline(function)) => match function.as_ref() {
            ModifierFunction::ExplorationMap(_) => true,
            ModifierFunction::Filtered(function) => {
                modifier_requires_resumption(compiler, function.on_pass.as_ref())
                    || modifier_requires_resumption(compiler, function.on_fail.as_ref())
            }
            ModifierFunction::Sequence(function) => {
                modifier_requires_resumption(compiler, Some(&function.functions))
            }
            _ => false,
        },
    }
}
