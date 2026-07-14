//! Vanilla `minecraft:custom_model_data` item component.

use std::io::{Cursor, Error, Read, Result, Write};

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::nbt::{NbtNumeric as _, nbt_collection_values};
use steel_utils::serial::{ReadFrom, WriteTo};

use super::rgb_color::decode_rgb_color;

const MAX_NETWORK_STRING_LENGTH: usize = 32_767;
const MAX_NETWORK_STRING_BYTES: usize = MAX_NETWORK_STRING_LENGTH * 3;
const MAX_INITIAL_LIST_CAPACITY: usize = 65_536;

/// Values exposed to item models and tint sources through custom model data.
#[derive(Debug, Clone)]
pub struct CustomModelData {
    floats: Vec<f32>,
    flags: Vec<bool>,
    strings: Vec<String>,
    colors: Vec<i32>,
}

impl CustomModelData {
    pub const EMPTY: Self = Self {
        floats: Vec::new(),
        flags: Vec::new(),
        strings: Vec::new(),
        colors: Vec::new(),
    };

    #[must_use]
    pub const fn new(
        floats: Vec<f32>,
        flags: Vec<bool>,
        strings: Vec<String>,
        colors: Vec<i32>,
    ) -> Self {
        Self {
            floats,
            flags,
            strings,
            colors,
        }
    }

    #[must_use]
    pub fn floats(&self) -> &[f32] {
        &self.floats
    }

    #[must_use]
    pub fn flags(&self) -> &[bool] {
        &self.flags
    }

    #[must_use]
    pub fn strings(&self) -> &[String] {
        &self.strings
    }

    #[must_use]
    pub fn colors(&self) -> &[i32] {
        &self.colors
    }

    #[must_use]
    pub fn get_float(&self, index: i32) -> Option<f32> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.floats.get(index))
            .copied()
    }

    #[must_use]
    pub fn get_boolean(&self, index: i32) -> Option<bool> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.flags.get(index))
            .copied()
    }

    #[must_use]
    pub fn get_string(&self, index: i32) -> Option<&str> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.strings.get(index))
            .map(String::as_str)
    }

    #[must_use]
    pub fn get_color(&self, index: i32) -> Option<i32> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.colors.get(index))
            .copied()
    }
}

impl Default for CustomModelData {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl PartialEq for CustomModelData {
    fn eq(&self, other: &Self) -> bool {
        self.floats.len() == other.floats.len()
            && self
                .floats
                .iter()
                .zip(&other.floats)
                .all(|(left, right)| float_equals(*left, *right))
            && self.flags == other.flags
            && self.strings == other.strings
            && self.colors == other.colors
    }
}

const fn float_equals(left: f32, right: f32) -> bool {
    (left.is_nan() && right.is_nan()) || left.to_bits() == right.to_bits()
}

impl WriteTo for CustomModelData {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_count(self.floats.len(), writer)?;
        for value in &self.floats {
            value.write(writer)?;
        }

        write_count(self.flags.len(), writer)?;
        for value in &self.flags {
            value.write(writer)?;
        }

        write_count(self.strings.len(), writer)?;
        for value in &self.strings {
            write_network_string(value, writer)?;
        }

        write_count(self.colors.len(), writer)?;
        for value in &self.colors {
            value.write(writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for CustomModelData {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let floats = read_list(data, f32::read)?;
        let flags = read_list(data, bool::read)?;
        let strings = read_list(data, read_network_string)?;
        let colors = read_list(data, i32::read)?;
        Ok(Self::new(floats, flags, strings, colors))
    }
}

fn write_count(count: usize, writer: &mut impl Write) -> Result<()> {
    let count = i32::try_from(count).map_err(|_| Error::other("List is too long"))?;
    VarInt(count).write(writer)
}

fn read_list<T>(
    data: &mut Cursor<&[u8]>,
    mut read_value: impl FnMut(&mut Cursor<&[u8]>) -> Result<T>,
) -> Result<Vec<T>> {
    let count = VarInt::read(data)?.0;
    let count = usize::try_from(count).map_err(|_| Error::other("Negative list length"))?;
    let mut values = Vec::with_capacity(count.min(MAX_INITIAL_LIST_CAPACITY));
    for _ in 0..count {
        values.push(read_value(data)?);
    }
    Ok(values)
}

fn write_network_string(value: &str, writer: &mut impl Write) -> Result<()> {
    if value.encode_utf16().count() > MAX_NETWORK_STRING_LENGTH {
        return Err(Error::other("String is longer than 32767 UTF-16 units"));
    }
    if value.len() > MAX_NETWORK_STRING_BYTES {
        return Err(Error::other("Encoded string is longer than 98301 bytes"));
    }
    write_count(value.len(), writer)?;
    writer.write_all(value.as_bytes())
}

fn read_network_string(data: &mut Cursor<&[u8]>) -> Result<String> {
    let byte_count = VarInt::read(data)?.0;
    let byte_count =
        usize::try_from(byte_count).map_err(|_| Error::other("Negative encoded string length"))?;
    if byte_count > MAX_NETWORK_STRING_BYTES {
        return Err(Error::other("Encoded string is longer than 98301 bytes"));
    }

    let mut bytes = vec![0; byte_count];
    data.read_exact(&mut bytes)?;
    let value = String::from_utf8_lossy(&bytes).into_owned();
    if value.encode_utf16().count() > MAX_NETWORK_STRING_LENGTH {
        return Err(Error::other("String is longer than 32767 UTF-16 units"));
    }
    Ok(value)
}

impl ToNbtTag for CustomModelData {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if !self.floats.is_empty() {
            compound.insert("floats", NbtList::Float(self.floats));
        }
        if !self.flags.is_empty() {
            compound.insert(
                "flags",
                NbtList::Byte(self.flags.into_iter().map(i8::from).collect()),
            );
        }
        if !self.strings.is_empty() {
            compound.insert(
                "strings",
                NbtList::String(self.strings.into_iter().map(Into::into).collect()),
            );
        }
        if !self.colors.is_empty() {
            compound.insert("colors", NbtList::Int(self.colors));
        }
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for CustomModelData {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag<'_, '_>) -> Option<Self> {
        let compound = tag.compound()?;
        let floats = decode_collection(compound.get("floats"), NbtTag::codec_f32)?;
        let flags = decode_collection(compound.get("flags"), NbtTag::codec_bool)?;
        let strings = decode_collection(compound.get("strings"), decode_string)?;
        let colors = decode_collection(compound.get("colors"), decode_rgb_color)?;
        Some(Self::new(floats, flags, strings, colors))
    }
}

fn decode_collection<T>(
    tag: Option<simdnbt::borrow::NbtTag<'_, '_>>,
    decode_value: impl Fn(&NbtTag) -> Option<T>,
) -> Option<Vec<T>> {
    let Some(tag) = tag else {
        return Some(Vec::new());
    };
    nbt_collection_values(&tag.to_owned())?
        .iter()
        .map(decode_value)
        .collect()
}

fn decode_string(tag: &NbtTag) -> Option<String> {
    let NbtTag::String(value) = tag else {
        return None;
    };
    value.to_owned().try_into_string().ok()
}

impl HashComponent for CustomModelData {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.clone().to_nbt_tag().hash_component(hasher);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::codec::VarInt;
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::CustomModelData;

    fn parse(tag: NbtTag) -> Option<CustomModelData> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        CustomModelData::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn persistent_codec_omits_default_lists() {
        assert_eq!(
            CustomModelData::default().to_nbt_tag(),
            NbtTag::Compound(NbtCompound::new())
        );
        assert_eq!(
            parse(NbtTag::Compound(NbtCompound::new())),
            Some(CustomModelData::EMPTY)
        );
    }

    #[test]
    fn persistent_codec_accepts_numeric_collections_and_rgb_vectors() {
        let mut compound = NbtCompound::new();
        compound.insert("floats", NbtTag::IntArray(vec![1, 2]));
        compound.insert("flags", NbtTag::ByteArray(vec![0, 2]));
        compound.insert("strings", NbtList::String(vec!["steel".into()]));
        compound.insert(
            "colors",
            NbtList::List(vec![NbtList::Float(vec![1.0, 0.5, 0.0])]),
        );

        let value = parse(NbtTag::Compound(compound)).expect("component should decode");
        assert_eq!(value.floats(), &[1.0, 2.0]);
        assert_eq!(value.flags(), &[false, true]);
        assert_eq!(value.strings(), &["steel"]);
        assert_eq!(value.colors(), &[0xffff_7f00_u32 as i32]);
    }

    #[test]
    fn network_codec_round_trips_all_lists() {
        let value = CustomModelData::new(
            vec![1.25, -0.0],
            vec![true, false],
            vec!["steel".to_owned(), "🦀".to_owned()],
            vec![0x123456, -1],
        );
        let mut encoded = Vec::new();
        value.write(&mut encoded).expect("component should encode");
        assert_eq!(
            CustomModelData::read(&mut Cursor::new(encoded.as_slice()))
                .expect("component should decode"),
            value
        );
    }

    #[test]
    fn network_codec_rejects_negative_counts_and_long_strings() {
        let mut negative_count = Vec::new();
        VarInt(-1)
            .write(&mut negative_count)
            .expect("count should encode");
        assert!(CustomModelData::read(&mut Cursor::new(negative_count.as_slice())).is_err());

        let value =
            CustomModelData::new(Vec::new(), Vec::new(), vec!["a".repeat(32_768)], Vec::new());
        assert!(value.write(&mut Vec::new()).is_err());
    }

    #[test]
    fn equality_matches_java_float_rules_and_getters_are_safe() {
        let left = CustomModelData::new(
            vec![f32::from_bits(0x7fc0_0001), 0.0],
            vec![true],
            vec!["value".to_owned()],
            vec![7],
        );
        let same = CustomModelData::new(
            vec![f32::from_bits(0x7fc0_0002), 0.0],
            vec![true],
            vec!["value".to_owned()],
            vec![7],
        );
        let negative_zero = CustomModelData::new(
            vec![f32::NAN, -0.0],
            vec![true],
            vec!["value".to_owned()],
            vec![7],
        );

        assert_eq!(left, same);
        assert_ne!(left, negative_zero);
        assert_eq!(left.get_float(-1), None);
        assert_eq!(left.get_boolean(0), Some(true));
        assert_eq!(left.get_string(0), Some("value"));
        assert_eq!(left.get_color(1), None);
    }

    #[test]
    fn persistent_hash_uses_the_record_codec_shape() {
        let value = CustomModelData::new(
            vec![1.25],
            vec![true],
            vec!["steel".to_owned()],
            vec![0x123456],
        );

        assert_eq!(
            value.compute_hash(),
            value.clone().to_nbt_tag().compute_hash()
        );
    }
}
