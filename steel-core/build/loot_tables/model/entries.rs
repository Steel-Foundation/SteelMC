pub use super::super::entries::{
    AlternativesEntry, DynamicEntry, EmptyEntry, GroupEntry, ItemEntry, LootTableEntry,
    SequenceEntry, TagEntry,
};
use super::super::{Compiler, entries::EntryExpansion};
use super::{Condition, Holder, Modifier};
use enum_dispatch::enum_dispatch;
use serde::Deserialize;

#[enum_dispatch]
pub(in super::super) trait CompileEntry {
    fn decorations(&self) -> (Option<&Holder<Condition>>, Option<&Modifier>);
    fn compile_expansion(&self, compiler: &mut Compiler) -> EntryExpansion;
}

// TODO: Add the vanilla slots entry codec and context item-slot access.
#[enum_dispatch(CompileEntry)]
#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum LootPoolEntry {
    #[serde(rename = "minecraft:item")]
    Item(ItemEntry),
    #[serde(rename = "minecraft:dynamic")]
    Dynamic(DynamicEntry),
    #[serde(rename = "minecraft:loot_table")]
    LootTable(LootTableEntry),
    #[serde(rename = "minecraft:tag")]
    Tag(TagEntry),
    #[serde(rename = "minecraft:empty")]
    Empty(EmptyEntry),
    #[serde(rename = "minecraft:alternatives")]
    Alternatives(AlternativesEntry),
    #[serde(rename = "minecraft:group")]
    Group(GroupEntry),
    #[serde(rename = "minecraft:sequence")]
    Sequence(SequenceEntry),
}
