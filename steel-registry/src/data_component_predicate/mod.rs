//! Registry-dispatched partial data-component predicates.

use std::fmt::{self, Debug, Formatter};
use std::io::{Cursor, Error, Result, Write};

use rustc_hash::{FxHashMap, FxHashSet};
use simdnbt::ToNbtTag as _;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry};
use steel_utils::nbt::NbtNumeric;
use steel_utils::serial::{ReadFrom, WriteTo};
use steel_utils::{Downcast as _, DowncastType, DowncastTypeKey, ErasedType, Identifier};
use text_components::TextComponent;

use crate::attribute::{Attribute, AttributeModifierOperation};
use crate::data_components::{ComponentData, ComponentEntryRef, DataComponentMap};
use crate::enchantment::Enchantment;
use crate::equipment::EquipmentSlotGroup;
use crate::item_predicate::{
    DoubleBounds, IntBounds, ItemPredicate, NbtPredicate, decode_optional, hash_entries,
    push_hash_entry, read_len, read_network_nbt, write_len,
};
use crate::jukebox_song::JukeboxSong;
use crate::potion::Potion;
use crate::trim_material::TrimMaterial;
use crate::trim_pattern::TrimPattern;
use crate::villager_type::VillagerType;
use crate::{REGISTRY, RegistryEntry, RegistryExt, RegistryHolderSet};

macro_rules! impl_predicate_downcast_type {
    ($type:ty, $key:literal) => {
        // SAFETY: This Steel-owned key uniquely identifies the concrete
        // predicate implementation within the process.
        unsafe impl DowncastType for $type {
            const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new($key);
        }
    };
}

/// Generic collection predicate shared by container, firework, book, and attribute checks.
mod attributes;
mod basic;
mod books;
mod collections;
mod core;
mod fireworks;
mod registry_predicates;
mod vanilla;

pub use attributes::*;
pub use basic::*;
pub use books::*;
pub use collections::*;
pub use core::*;
pub use fireworks::*;
pub use registry_predicates::*;
pub use vanilla::*;

#[cfg(test)]
mod tests;
