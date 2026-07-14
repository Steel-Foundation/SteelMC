//! Vanilla `CustomData` item component value.

use std::io::{Cursor, Error, Result, Write};

use simdnbt::owned::{NbtCompound, NbtTag, read_tag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::{
    hash::{ComponentHasher, HashComponent},
    nbt::{
        compare_nbt_compounds, merge_nbt_compounds, normalize_nbt_compound, vanilla_nbt_heap_size,
    },
    serial::{ReadFrom, WriteTo},
};

const DEFAULT_NBT_QUOTA: u64 = 2_097_152;

/// Immutable component wrapper around a Vanilla-normalized NBT compound.
#[derive(Debug, Clone)]
pub struct CustomData {
    tag: NbtCompound,
}

impl CustomData {
    /// Creates a component after validating strings and applying Vanilla map
    /// semantics to duplicate compound keys.
    #[must_use]
    pub fn try_from_compound(tag: NbtCompound) -> Option<Self> {
        normalize_nbt_compound(tag).map(|tag| Self { tag })
    }

    /// Decodes the persistent codec's compound-or-flattened-SNBT alternatives.
    #[must_use]
    pub fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        Self::from_codec_tag(tag.clone())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tag.is_empty()
    }

    #[must_use]
    pub fn copy_tag(&self) -> NbtCompound {
        self.tag.clone()
    }

    #[must_use]
    pub const fn as_compound(&self) -> &NbtCompound {
        &self.tag
    }

    pub(crate) fn without_field(mut self, name: &str) -> Self {
        self.tag.remove(name);
        self
    }

    /// Mirrors `CustomData.matchedBy` and `NbtUtils.compareNbt`.
    #[must_use]
    pub fn matched_by(&self, expected: &NbtCompound) -> bool {
        compare_nbt_compounds(expected, &self.tag, true)
    }

    /// Returns a copy updated through Vanilla `CompoundTag.merge` semantics.
    #[must_use]
    pub fn merged_with(&self, other: &Self) -> Self {
        let mut tag = self.tag.clone();
        merge_nbt_compounds(&mut tag, &other.tag);
        Self { tag }
    }

    pub(crate) fn read_codec_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Self::from_codec_tag(read_network_tag(data)?).ok_or_else(|| {
            Error::other("Custom data network value is not a compound or SNBT string")
        })
    }

    fn from_codec_tag(tag: NbtTag) -> Option<Self> {
        match tag {
            NbtTag::Compound(compound) => Self::try_from_compound(compound),
            NbtTag::String(value) => {
                let value = value.try_into_string().ok()?;
                let compound = steel_utils::nbt::parse_snbt_compound(&value).ok()?;
                Self::try_from_compound(compound)
            }
            _ => None,
        }
    }
}

impl Default for CustomData {
    fn default() -> Self {
        Self {
            tag: NbtCompound::new(),
        }
    }
}

impl PartialEq for CustomData {
    fn eq(&self, other: &Self) -> bool {
        steel_utils::nbt::nbt_compounds_equal(&self.tag, &other.tag)
    }
}

impl WriteTo for CustomData {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let mut encoded = Vec::new();
        NbtTag::Compound(self.tag.clone()).write(&mut encoded);
        writer.write_all(&encoded)
    }
}

impl ReadFrom for CustomData {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let NbtTag::Compound(compound) = read_network_tag(data)? else {
            return Err(Error::other(
                "Bucket entity data network value is not a compound",
            ));
        };
        Self::try_from_compound(compound)
            .ok_or_else(|| Error::other("Bucket entity data contains malformed modified UTF-8"))
    }
}

fn read_network_tag(data: &mut Cursor<&[u8]>) -> Result<NbtTag> {
    let tag = read_tag(data).map_err(|error| Error::other(format!("Invalid NBT: {error:?}")))?;
    let Some(heap_size) = vanilla_nbt_heap_size(&tag) else {
        return Err(Error::other("NBT contains malformed modified UTF-8"));
    };
    if heap_size > DEFAULT_NBT_QUOTA {
        return Err(Error::other(format!(
            "NBT exceeds Vanilla's {DEFAULT_NBT_QUOTA}-byte heap quota"
        )));
    }
    Ok(tag)
}

impl ToNbtTag for CustomData {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::Compound(self.tag)
    }
}

impl FromNbtTag for CustomData {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag<'_, '_>) -> Option<Self> {
        Self::from_codec_tag(tag.to_owned())
    }
}

impl HashComponent for CustomData {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        NbtTag::Compound(self.tag.clone()).hash_component(hasher);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
    use steel_utils::{
        hash::HashComponent as _,
        serial::{ReadFrom as _, WriteTo as _},
    };

    use super::CustomData;

    fn sample_compound() -> NbtCompound {
        let mut nested = NbtCompound::new();
        nested.insert("name", "steel");
        let mut compound = NbtCompound::new();
        compound.insert("value", 7);
        compound.insert("nested", nested);
        compound
    }

    #[test]
    fn persistent_codec_accepts_compounds_and_flattened_snbt() {
        let direct = CustomData::from_nbt_value(&NbtTag::Compound(sample_compound()))
            .expect("compound should decode");
        let flattened =
            CustomData::from_nbt_value(&NbtTag::String("{value:7,nested:{name:'steel'}}".into()))
                .expect("flattened SNBT should decode");

        assert_eq!(direct, flattened);
        assert!(CustomData::from_nbt_value(&NbtTag::Int(7)).is_none());
    }

    #[test]
    fn network_codecs_differ_only_on_the_flattened_alternative() {
        let value = CustomData::try_from_compound(sample_compound())
            .expect("sample compound should be valid");
        let mut encoded = Vec::new();
        value
            .write(&mut encoded)
            .expect("custom data should encode");

        assert_eq!(
            CustomData::read(&mut Cursor::new(encoded.as_slice()))
                .expect("raw compound stream should decode"),
            value
        );
        assert_eq!(
            CustomData::read_codec_network(&mut Cursor::new(encoded.as_slice()))
                .expect("codec-derived stream should decode"),
            value
        );

        let mut flattened = Vec::new();
        NbtTag::String("{value:7}".into()).write(&mut flattened);
        assert!(CustomData::read(&mut Cursor::new(flattened.as_slice())).is_err());
        assert!(CustomData::read_codec_network(&mut Cursor::new(flattened.as_slice())).is_ok());
    }

    #[test]
    fn network_decode_enforces_vanilla_nbt_heap_quota() {
        let mut compound = NbtCompound::new();
        compound.insert("values", NbtList::String(vec!["".into(); 60_000]));
        let mut encoded = Vec::new();
        NbtTag::Compound(compound).write(&mut encoded);

        assert!(CustomData::read_codec_network(&mut Cursor::new(encoded.as_slice())).is_err());
    }

    #[test]
    fn persistent_hash_uses_the_compound_codec_shape() {
        let compound = sample_compound();
        let value = CustomData::try_from_compound(compound.clone())
            .expect("sample compound should be valid");

        assert_eq!(
            value.compute_hash(),
            NbtTag::Compound(compound).compute_hash()
        );
    }
}
