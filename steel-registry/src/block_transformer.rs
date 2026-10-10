//! `minecraft:block_transformer` registry entries.
//!
//! Vanilla moved this from an inline per-item component to a proper dynamic
//! registry (`Registries.BLOCK_TRANSFORMER`) referenced by items via
//! `Holder<BlockTransformer>`. Entries are built from datapack JSON at build
//! time; the item component itself only stores a registry reference.

use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::{NbtList, NbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{ReadFrom, WriteTo};
use steel_utils::{Direction, Identifier};

use crate::feature::BlockStateProviderKind;
use crate::sound_event::SoundEventHolder;
use crate::{REGISTRY, RegistryEntry, RegistryExt};

/// Item block transforms, e.g. shovel flattening dirt into a path.
///
/// `PartialEq`/`Eq` come from [`crate::impl_registry_entry!`] (identity by key).
#[derive(Debug, Clone)]
pub struct BlockTransformer {
    pub key: Identifier,
    pub transforms: Vec<BlockTransformData>,
    pub nbt: fn() -> NbtList,
}

#[derive(Debug, Clone)]
pub struct BlockTransformData {
    pub block_state_provider: BlockStateProviderKind,
    pub sound: SoundEventHolder,
    pub particle: TransformParticle,
    pub disallowed_faces: Vec<Direction>,
    pub loot: Option<Identifier>,
    pub drop_strategy: DropStrategy,
    pub update_from_neighbors: bool,
    pub transform_type: TransformType,
    pub consume_on_use: bool,
    pub item_damage_per_use: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransformParticle {
    #[default]
    None,
    Scrape,
    WaxOn,
    WaxOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DropStrategy {
    ClickedFace,
    #[default]
    FromMiddle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransformType {
    #[default]
    SingleBlock,
    CopperChest,
}

/// Registry-sync NBT for the `minecraft:block_transformer` dynamic registry.
/// Only encoding is needed: entries are build-time data, never decoded from
/// player-supplied NBT or network input.
impl ToNbtTag for &BlockTransformer {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::List((self.nbt)())
    }
}

pub type BlockTransformerRef = &'static BlockTransformer;

/// Item component wrapper: a `Holder<BlockTransformer>` reference, wire- and
/// NBT-encoded like other registry holders (e.g. [`crate::damage_type::DamageTypeComponent`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockTransformerComponent {
    pub block_transformer: BlockTransformerRef,
}

impl BlockTransformerComponent {
    #[must_use]
    pub const fn new(block_transformer: BlockTransformerRef) -> Self {
        Self { block_transformer }
    }
}

impl WriteTo for BlockTransformerComponent {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let id = self.block_transformer.try_id().ok_or_else(|| {
            Error::other(format!(
                "Unknown block transformer: {}",
                self.block_transformer.key
            ))
        })?;
        let id = i32::try_from(id).map_err(|_| {
            Error::other(format!("Block transformer id out of protocol range: {id}"))
        })?;
        VarInt(id).write(writer)
    }
}

impl ReadFrom for BlockTransformerComponent {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let id = VarInt::read(data)?.0;
        let id = usize::try_from(id)
            .map_err(|_| Error::other(format!("Negative block transformer id: {id}")))?;
        let block_transformer = REGISTRY
            .block_transformers
            .by_id(id)
            .ok_or_else(|| Error::other(format!("Unknown block transformer id: {id}")))?;
        Ok(Self { block_transformer })
    }
}

impl simdnbt::ToNbtTag for BlockTransformerComponent {
    fn to_nbt_tag(self) -> NbtTag {
        self.block_transformer.key.to_string().to_nbt_tag()
    }
}

impl simdnbt::FromNbtTag for BlockTransformerComponent {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let key = Identifier::from_str(&tag.string()?.to_str()).ok()?;
        REGISTRY
            .block_transformers
            .by_key(&key)
            .map(|block_transformer| Self { block_transformer })
    }
}

impl HashComponent for BlockTransformerComponent {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_string(&self.block_transformer.key.to_string());
    }
}

pub struct BlockTransformerRegistry {
    entries_by_id: Vec<BlockTransformerRef>,
    entries_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl BlockTransformerRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries_by_id: Vec::new(),
            entries_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    BlockTransformerRegistry,
    BlockTransformerRef,
    entries_by_id,
    entries_by_key,
    allows_registering
);

crate::impl_registry!(
    BlockTransformerRegistry,
    BlockTransformer,
    entries_by_id,
    entries_by_key,
    block_transformers
);

#[cfg(test)]
mod tests;
