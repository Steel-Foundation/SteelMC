//! Lenient numeric reads of NBT compounds.

use simdnbt::{
    borrow::{NbtCompound as BorrowedNbtCompound, NbtTag as BorrowedNbtTag},
    owned::{NbtCompound as OwnedNbtCompound, NbtTag as OwnedNbtTag},
};

/// A numeric NBT tag that converts to any numeric type.
///
/// Integer conversions floor floating point tags, while the `codec_*` methods
/// of [`NbtNumeric`](super::NbtNumeric) truncate them. Use this type for
/// `getIntOr`-style reads and `NbtNumeric` for `Codec.INT`-style reads.
#[derive(Clone, Copy, Debug, PartialEq)]
#[expect(missing_docs, reason = "each variant is the NBT tag of that name")]
pub enum NumericTag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
}

impl NumericTag {
    const fn floor(value: f64) -> i32 {
        value.floor() as i32
    }

    /// Keeps the low bits of the integer value, flooring floating point tags first.
    #[must_use]
    pub fn byte_value(self) -> i8 {
        match self {
            Self::Byte(value) => value,
            Self::Short(value) => value as i8,
            Self::Int(value) => value as i8,
            Self::Long(value) => value as i8,
            Self::Float(value) => Self::floor(f64::from(value)) as i8,
            Self::Double(value) => Self::floor(value) as i8,
        }
    }

    /// Keeps the low bits of the integer value, flooring floating point tags first.
    #[must_use]
    pub fn short_value(self) -> i16 {
        match self {
            Self::Byte(value) => i16::from(value),
            Self::Short(value) => value,
            Self::Int(value) => value as i16,
            Self::Long(value) => value as i16,
            Self::Float(value) => Self::floor(f64::from(value)) as i16,
            Self::Double(value) => Self::floor(value) as i16,
        }
    }

    /// Keeps the low bits of long tags and floors floating point tags.
    #[must_use]
    pub fn int_value(self) -> i32 {
        match self {
            Self::Byte(value) => i32::from(value),
            Self::Short(value) => i32::from(value),
            Self::Int(value) => value,
            Self::Long(value) => value as i32,
            Self::Float(value) => Self::floor(f64::from(value)),
            Self::Double(value) => Self::floor(value),
        }
    }

    /// Truncates float tags but floors double tags, as vanilla does.
    #[must_use]
    pub fn long_value(self) -> i64 {
        match self {
            Self::Byte(value) => i64::from(value),
            Self::Short(value) => i64::from(value),
            Self::Int(value) => i64::from(value),
            Self::Long(value) => value,
            Self::Float(value) => value as i64,
            Self::Double(value) => value.floor() as i64,
        }
    }

    /// Converts the value to the nearest `f32`.
    #[must_use]
    pub fn float_value(self) -> f32 {
        match self {
            Self::Byte(value) => f32::from(value),
            Self::Short(value) => f32::from(value),
            Self::Int(value) => value as f32,
            Self::Long(value) => value as f32,
            Self::Float(value) => value,
            Self::Double(value) => value as f32,
        }
    }

    /// Converts the value to the nearest `f64`.
    #[must_use]
    pub fn double_value(self) -> f64 {
        match self {
            Self::Byte(value) => f64::from(value),
            Self::Short(value) => f64::from(value),
            Self::Int(value) => f64::from(value),
            Self::Long(value) => value as f64,
            Self::Float(value) => f64::from(value),
            Self::Double(value) => value,
        }
    }
}

impl TryFrom<&OwnedNbtTag> for NumericTag {
    type Error = ();

    fn try_from(tag: &OwnedNbtTag) -> Result<Self, Self::Error> {
        match tag {
            OwnedNbtTag::Byte(value) => Ok(Self::Byte(*value)),
            OwnedNbtTag::Short(value) => Ok(Self::Short(*value)),
            OwnedNbtTag::Int(value) => Ok(Self::Int(*value)),
            OwnedNbtTag::Long(value) => Ok(Self::Long(*value)),
            OwnedNbtTag::Float(value) => Ok(Self::Float(*value)),
            OwnedNbtTag::Double(value) => Ok(Self::Double(*value)),
            _ => Err(()),
        }
    }
}

impl TryFrom<&BorrowedNbtTag<'_, '_>> for NumericTag {
    type Error = ();

    fn try_from(tag: &BorrowedNbtTag<'_, '_>) -> Result<Self, Self::Error> {
        tag.byte()
            .map(Self::Byte)
            .or_else(|| tag.short().map(Self::Short))
            .or_else(|| tag.int().map(Self::Int))
            .or_else(|| tag.long().map(Self::Long))
            .or_else(|| tag.float().map(Self::Float))
            .or_else(|| tag.double().map(Self::Double))
            .ok_or(())
    }
}

/// Numeric reads accepting any numeric tag, unlike simdnbt's getters which
/// return `None` when the tag type differs.
///
/// `getIntOr(name, default)` is `int_value(name).unwrap_or(default)` here, and
/// `getBooleanOr(name, false)` is `byte_value(name).is_some_and(|v| v != 0)`.
pub trait NbtValueInput {
    /// Returns the tag called `name` if it exists and is numeric.
    fn numeric_tag(&self, name: &str) -> Option<NumericTag>;

    /// Reads `name` as an `i8`, see [`NumericTag::byte_value`].
    fn byte_value(&self, name: &str) -> Option<i8> {
        self.numeric_tag(name).map(NumericTag::byte_value)
    }

    /// Reads `name` as an `i16`, see [`NumericTag::short_value`].
    fn short_value(&self, name: &str) -> Option<i16> {
        self.numeric_tag(name).map(NumericTag::short_value)
    }

    /// Reads `name` as an `i32`, see [`NumericTag::int_value`].
    fn int_value(&self, name: &str) -> Option<i32> {
        self.numeric_tag(name).map(NumericTag::int_value)
    }

    /// Reads `name` as an `i64`, see [`NumericTag::long_value`].
    fn long_value(&self, name: &str) -> Option<i64> {
        self.numeric_tag(name).map(NumericTag::long_value)
    }

    /// Reads `name` as an `f32`, see [`NumericTag::float_value`].
    fn float_value(&self, name: &str) -> Option<f32> {
        self.numeric_tag(name).map(NumericTag::float_value)
    }

    /// Reads `name` as an `f64`, see [`NumericTag::double_value`].
    fn double_value(&self, name: &str) -> Option<f64> {
        self.numeric_tag(name).map(NumericTag::double_value)
    }
}

impl NbtValueInput for OwnedNbtCompound {
    fn numeric_tag(&self, name: &str) -> Option<NumericTag> {
        self.get(name)?.try_into().ok()
    }
}

impl NbtValueInput for BorrowedNbtCompound<'_, '_> {
    fn numeric_tag(&self, name: &str) -> Option<NumericTag> {
        (&self.get(name)?).try_into().ok()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::owned::{NbtCompound, NbtTag};

    use super::{NbtValueInput as _, NumericTag};

    #[test]
    fn integer_tags_narrow_by_keeping_low_bits() {
        assert_eq!(NumericTag::Short(0x1FF).byte_value(), -1);
        assert_eq!(NumericTag::Int(0x1_FFFF).short_value(), -1);
        assert_eq!(
            NumericTag::Long(i64::from(i32::MAX) + 1).int_value(),
            i32::MIN
        );
        assert_eq!(NumericTag::Long(0x1FF).byte_value(), -1);
    }

    #[test]
    fn floating_tags_floor_into_integers() {
        assert_eq!(NumericTag::Float(-1.5).int_value(), -2);
        assert_eq!(NumericTag::Double(-1.5).int_value(), -2);
        assert_eq!(NumericTag::Float(5.9).int_value(), 5);
        assert_eq!(NumericTag::Double(-0.5).short_value(), -1);
        assert_eq!(NumericTag::Float(-0.5).byte_value(), -1);
    }

    #[test]
    fn float_long_value_truncates_but_double_floors() {
        assert_eq!(NumericTag::Float(-1.5).long_value(), -1);
        assert_eq!(NumericTag::Double(-1.5).long_value(), -2);
    }

    #[test]
    fn floating_tags_saturate_before_narrowing() {
        assert_eq!(NumericTag::Double(1e300).int_value(), i32::MAX);
        assert_eq!(NumericTag::Double(1e300).byte_value(), -1);
        assert_eq!(NumericTag::Float(f32::NAN).int_value(), 0);
    }

    #[test]
    fn compound_reads_accept_any_numeric_tag() {
        let mut nbt = NbtCompound::new();
        nbt.insert("Radius", 1);
        nbt.insert("Duration", 200i16);
        nbt.insert("Name", "1");

        assert_eq!(nbt.float_value("Radius"), Some(1.0));
        assert_eq!(nbt.int_value("Duration"), Some(200));
        assert_eq!(nbt.int_value("Name"), None);
        assert_eq!(nbt.int_value("Missing"), None);
    }

    #[test]
    fn borrowed_compound_reads_accept_any_numeric_tag() {
        let mut nbt = NbtCompound::new();
        nbt.insert("Radius", 1);
        let mut bytes = Vec::new();
        NbtTag::Compound(nbt).write(&mut bytes);
        let borrowed =
            read_tag(&mut Cursor::new(bytes.as_slice())).expect("owned test tag should parse");
        let tag = borrowed.as_tag();
        let compound = tag.compound().expect("test tag is a compound");

        assert_eq!(compound.float_value("Radius"), Some(1.0));
    }
}
