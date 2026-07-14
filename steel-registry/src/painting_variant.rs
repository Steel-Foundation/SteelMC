use crate::{REGISTRY, RegistryExt, RegistryHolderEntry};
use rustc_hash::FxHashMap;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use std::io::{Cursor, Result, Write};
use steel_utils::Identifier;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};
use text_components::TextComponent;

#[derive(Debug, Clone, PartialEq)]
pub struct PaintingVariantValue {
    pub width: i32,
    pub height: i32,
    pub asset_id: Identifier,
    pub title: Option<TextComponent>,
    pub author: Option<TextComponent>,
}

impl WriteTo for PaintingVariantValue {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        steel_utils::codec::VarInt(self.width).write(writer)?;
        steel_utils::codec::VarInt(self.height).write(writer)?;
        self.asset_id.write(writer)?;
        self.title.write(writer)?;
        self.author.write(writer)
    }
}

impl ReadFrom for PaintingVariantValue {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            width: steel_utils::codec::VarInt::read(data)?.0,
            height: steel_utils::codec::VarInt::read(data)?.0,
            asset_id: Identifier::read(data)?,
            title: Option::<TextComponent>::read(data)?,
            author: Option::<TextComponent>::read(data)?,
        })
    }
}

impl ToNbtTag for PaintingVariantValue {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("asset_id", self.asset_id.to_string());
        compound.insert("width", self.width);
        compound.insert("height", self.height);
        if let Some(title) = self.title {
            compound.insert("title", title.to_codec_nbt());
        }
        if let Some(author) = self.author {
            compound.insert("author", author.to_codec_nbt());
        }
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for PaintingVariantValue {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let width = compound.get("width")?.codec_i32()?;
        let height = compound.get("height")?.codec_i32()?;
        if !(1..=16).contains(&width) || !(1..=16).contains(&height) {
            return None;
        }
        Some(Self {
            width,
            height,
            asset_id: Identifier::from_nbt_tag(compound.get("asset_id")?)?,
            title: match compound.get("title") {
                Some(tag) => Some(TextComponent::from_nbt(&tag.to_owned())?),
                None => None,
            },
            author: match compound.get("author") {
                Some(tag) => Some(TextComponent::from_nbt(&tag.to_owned())?),
                None => None,
            },
        })
    }
}

impl HashComponent for PaintingVariantValue {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.clone().to_nbt_tag().hash_component(hasher);
    }
}

/// Represents a painting variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct PaintingVariant {
    pub key: Identifier,
    value: PaintingVariantValue,
}

impl PaintingVariant {
    #[must_use]
    pub const fn new(key: Identifier, value: PaintingVariantValue) -> Self {
        Self { key, value }
    }
    #[must_use]
    pub const fn value(&self) -> &PaintingVariantValue {
        &self.value
    }
}

impl ToNbtTag for &PaintingVariant {
    fn to_nbt_tag(self) -> NbtTag {
        self.value.clone().to_nbt_tag()
    }
}

pub type PaintingVariantRef = &'static PaintingVariant;

pub struct PaintingVariantRegistry {
    painting_variants_by_id: Vec<PaintingVariantRef>,
    painting_variants_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl PaintingVariantRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            painting_variants_by_id: Vec::new(),
            painting_variants_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    PaintingVariantRegistry,
    PaintingVariantRef,
    painting_variants_by_id,
    painting_variants_by_key,
    allows_registering
);

crate::impl_registry!(
    PaintingVariantRegistry,
    PaintingVariant,
    painting_variants_by_id,
    painting_variants_by_key,
    painting_variants
);
crate::impl_tagged_registry!(
    PaintingVariantRegistry,
    painting_variants_by_key,
    "painting variant"
);

impl RegistryHolderEntry for PaintingVariant {
    type Value = PaintingVariantValue;
    const REGISTRY_NAME: &'static str = "painting variant";
    fn holder_value(&self) -> &Self::Value {
        &self.value
    }
    fn holder_by_id(id: usize) -> Option<&'static Self> {
        REGISTRY.painting_variants.by_id(id)
    }
    fn holder_by_key(key: &Identifier) -> Option<&'static Self> {
        REGISTRY.painting_variants.by_key(key)
    }
}
